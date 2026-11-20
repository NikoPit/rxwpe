//! Project-local resources and Wallpaper Engine package mounts.
//!
//! Lookup order is loose project files, project packages, then shared resources.
//! Package entries are read in place, without extracting files to disk.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};

const MAX_ENTRIES: u32 = 1_000_000;
const MAX_NAME_BYTES: u32 = 16 * 1024;
const MAX_READ_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug)]
pub(crate) struct AssetStore {
    root: PathBuf,
    shared: Option<PathBuf>,
    packages: Vec<Package>,
}

#[derive(Debug)]
struct Package {
    path: PathBuf,
    entries: BTreeMap<String, Entry>,
}

#[derive(Debug, Clone, Copy)]
struct Entry {
    offset: u64,
    length: u64,
}

impl AssetStore {
    pub(crate) fn open(project_dir: &Path, shared_assets: Option<&Path>) -> Result<Self> {
        let root = project_dir
            .canonicalize()
            .with_context(|| format!("cannot open project directory {}", project_dir.display()))?;
        ensure!(
            root.is_dir(),
            "project path is not a directory: {}",
            root.display()
        );
        let shared = shared_assets
            .map(|p| {
                let path = p
                    .canonicalize()
                    .with_context(|| format!("cannot open shared assets {}", p.display()))?;
                ensure!(
                    path.is_dir(),
                    "shared assets path is not a directory: {}",
                    path.display()
                );
                Ok::<_, anyhow::Error>(path)
            })
            .transpose()?;
        let mut packages = Vec::new();
        for name in ["scene.pkg", "gifscene.pkg"] {
            let path = root.join(name);
            if path.exists() {
                let path = contained_file(&root, name)?;
                packages.push(
                    Package::open(&path)
                        .with_context(|| format!("invalid package {}", path.display()))?,
                );
            }
        }
        Ok(Self {
            root,
            shared,
            packages,
        })
    }

    pub(crate) fn read(&self, path: &str) -> Result<Vec<u8>> {
        let name = normalize_path(path)?;
        if self.root.join(&name).exists() {
            return read_file(&contained_file(&self.root, &name)?);
        }
        for package in &self.packages {
            if let Some(entry) = package.entries.get(&name) {
                ensure!(
                    entry.length <= MAX_READ_BYTES,
                    "asset exceeds 512 MiB read limit: {name}"
                );
                let mut file = File::open(&package.path)?;
                file.seek(SeekFrom::Start(entry.offset))?;
                let mut bytes = vec![0; usize::try_from(entry.length)?];
                file.read_exact(&mut bytes)
                    .with_context(|| format!("cannot read package asset {name}"))?;
                return Ok(bytes);
            }
        }
        if let Some(shared) = self.shared.as_ref().filter(|dir| dir.join(&name).exists()) {
            return read_file(&contained_file(shared, &name)?);
        }
        bail!("asset not found: {name}");
    }

    pub(crate) fn exists(&self, path: &str) -> bool {
        let Ok(name) = normalize_path(path) else {
            return false;
        };
        if self.root.join(&name).exists() {
            return contained_file(&self.root, &name).is_ok();
        }
        if self.packages.iter().any(|p| p.entries.contains_key(&name)) {
            return true;
        }
        self.shared
            .as_ref()
            .is_some_and(|p| contained_file(p, &name).is_ok())
    }

    /// Returns a filesystem path only when the highest-priority asset is loose.
    pub(crate) fn physical_path(&self, path: &str) -> Result<PathBuf> {
        let name = normalize_path(path)?;
        if self.root.join(&name).exists() {
            return contained_file(&self.root, &name);
        }
        if self.packages.iter().any(|p| p.entries.contains_key(&name)) {
            bail!("asset is inside a package and has no physical path: {name}");
        }
        if let Some(shared) = self.shared.as_ref().filter(|dir| dir.join(&name).exists()) {
            return contained_file(shared, &name);
        }
        bail!("asset not found: {name}");
    }
}

pub(crate) fn normalize_path(path: &str) -> Result<String> {
    ensure!(
        !path.is_empty() && !path.contains(['\0', ':']),
        "invalid asset path: {path:?}"
    );
    let path = path.replace('\\', "/");
    ensure!(
        !path.starts_with('/'),
        "absolute asset paths are not allowed: {path}"
    );
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => bail!("parent traversal is not allowed in asset paths: {path}"),
            _ => parts.push(part),
        }
    }
    ensure!(!parts.is_empty(), "empty asset path");
    Ok(parts.join("/"))
}

fn contained_file(root: &Path, name: &str) -> Result<PathBuf> {
    let path = root
        .join(name)
        .canonicalize()
        .with_context(|| format!("cannot open asset {name}"))?;
    ensure!(
        path.starts_with(root),
        "asset escapes its mount directory: {name}"
    );
    ensure!(path.is_file(), "asset is not a regular file: {name}");
    Ok(path)
}

fn read_file(path: &Path) -> Result<Vec<u8>> {
    let mut file = File::open(path)?;
    ensure!(
        file.metadata()?.len() <= MAX_READ_BYTES,
        "asset exceeds 512 MiB read limit: {}",
        path.display()
    );
    let mut bytes = Vec::new();
    // Bound the read even if another process grows the file after metadata().
    file.by_ref()
        .take(MAX_READ_BYTES + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_READ_BYTES,
        "asset exceeds 512 MiB read limit"
    );
    Ok(bytes)
}

impl Package {
    fn open(path: &Path) -> Result<Self> {
        let mut file = File::open(path)?;
        let size = file.metadata()?.len();
        let version = read_string(&mut file)?;
        ensure!(
            version.starts_with("PKGV"),
            "expected PKGV package header, found {version:?}"
        );
        let count = read_u32(&mut file)?;
        ensure!(count <= MAX_ENTRIES, "package has too many files: {count}");
        ensure!(
            u64::from(count) <= size.saturating_sub(file.stream_position()?) / 12,
            "package file count exceeds available index bytes"
        );
        let mut relative_entries = BTreeMap::new();
        for _ in 0..count {
            let name = normalize_path(&read_string(&mut file)?)?;
            let entry = Entry {
                offset: read_u32(&mut file)?.into(),
                length: read_u32(&mut file)?.into(),
            };
            ensure!(
                relative_entries.insert(name.clone(), entry).is_none(),
                "duplicate package entry: {name}"
            );
        }
        let data_start = file.stream_position()?;
        let mut entries = BTreeMap::new();
        for (name, entry) in relative_entries {
            let offset = data_start
                .checked_add(entry.offset)
                .context("package offset overflow")?;
            let end = offset
                .checked_add(entry.length)
                .context("package entry size overflow")?;
            ensure!(end <= size, "package entry exceeds file bounds: {name}");
            entries.insert(
                name,
                Entry {
                    offset,
                    length: entry.length,
                },
            );
        }
        Ok(Self {
            path: path.to_path_buf(),
            entries,
        })
    }
}

fn read_u32(file: &mut File) -> Result<u32> {
    let mut bytes = [0; 4];
    file.read_exact(&mut bytes)
        .context("truncated package index")?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_string(file: &mut File) -> Result<String> {
    let length = read_u32(file)?;
    ensure!(
        length <= MAX_NAME_BYTES,
        "package string exceeds 16 KiB limit"
    );
    let mut bytes = vec![0; length as usize];
    file.read_exact(&mut bytes)
        .context("truncated package string")?;
    String::from_utf8(bytes).context("package filename is not UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    struct TestDir(PathBuf);
    impl TestDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "rxwpe-assets-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn package(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend(8u32.to_le_bytes());
        out.extend(b"PKGV0001");
        out.extend((entries.len() as u32).to_le_bytes());
        let mut offset = 0u32;
        for (name, data) in entries {
            out.extend((name.len() as u32).to_le_bytes());
            out.extend(name.as_bytes());
            out.extend(offset.to_le_bytes());
            out.extend((data.len() as u32).to_le_bytes());
            offset += data.len() as u32;
        }
        for (_, data) in entries {
            out.extend(*data);
        }
        out
    }
    #[test]
    fn reads_package_offsets_and_mount_precedence() {
        let root = TestDir::new();
        let shared = TestDir::new();
        fs::write(
            root.0.join("scene.pkg"),
            package(&[("a.txt", b"package"), ("nested/b.txt", b"second")]),
        )
        .unwrap();
        fs::write(root.0.join("a.txt"), b"loose").unwrap();
        fs::write(shared.0.join("a.txt"), b"shared").unwrap();
        fs::write(shared.0.join("c.txt"), b"fallback").unwrap();
        let store = AssetStore::open(&root.0, Some(&shared.0)).unwrap();
        assert_eq!(store.read("a.txt").unwrap(), b"loose");
        assert_eq!(store.read("nested\\b.txt").unwrap(), b"second");
        assert_eq!(store.read("c.txt").unwrap(), b"fallback");
        assert!(store.physical_path("nested/b.txt").is_err());
    }
    #[test]
    fn rejects_truncated_and_out_of_bounds_packages() {
        let root = TestDir::new();
        let mut bytes = package(&[("a", b"hello")]);
        bytes.pop();
        fs::write(root.0.join("scene.pkg"), bytes).unwrap();
        assert!(
            AssetStore::open(&root.0, None)
                .unwrap_err()
                .to_string()
                .contains("invalid package")
        );
        fs::write(root.0.join("scene.pkg"), [0xff; 12]).unwrap();
        assert!(AssetStore::open(&root.0, None).is_err());
    }
    #[test]
    fn rejects_unsafe_and_duplicate_entry_paths() {
        for entries in [
            vec![("../escape", &b"x"[..])],
            vec![("a", &b"x"[..]), ("a", &b"y"[..])],
        ] {
            let root = TestDir::new();
            fs::write(root.0.join("scene.pkg"), package(&entries)).unwrap();
            assert!(AssetStore::open(&root.0, None).is_err());
        }
        for path in ["../escape", "/absolute", "C:\\absolute", "a/../../b", ""] {
            assert!(normalize_path(path).is_err(), "{path}");
        }
    }
    #[cfg(unix)]
    #[test]
    fn loose_symlinks_cannot_escape_mount() {
        let root = TestDir::new();
        let other = TestDir::new();
        fs::write(other.0.join("outside"), b"secret").unwrap();
        std::os::unix::fs::symlink(other.0.join("outside"), root.0.join("linked")).unwrap();
        let store = AssetStore::open(&root.0, None).unwrap();
        assert!(store.read("linked").is_err());
        assert!(!store.exists("linked"));
    }
}
