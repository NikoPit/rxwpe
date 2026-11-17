use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

pub fn libraries(explicit: &[PathBuf]) -> Vec<PathBuf> {
    let mut result = explicit.to_vec();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        for relative in default_library_paths() {
            let root = home.join(relative);
            if !result.contains(&root) {
                result.push(root);
            }
        }
    }
    result
}

fn default_library_paths() -> &'static [&'static str] {
    #[cfg(target_os = "macos")]
    {
        &["Library/Application Support/Steam"]
    }
    #[cfg(not(target_os = "macos"))]
    {
        &[
            ".local/share/Steam",
            ".steam/steam",
            ".steam/root",
            ".var/app/com.valvesoftware.Steam/.local/share/Steam",
        ]
    }
}

fn steamapps(root: &Path) -> PathBuf {
    if root.file_name().is_some_and(|name| name == "steamapps") {
        root.to_owned()
    } else {
        root.join("steamapps")
    }
}

pub fn resolve(input: &str, libraries: &[PathBuf]) -> Result<PathBuf> {
    let path = Path::new(input);
    if path.is_dir() {
        return Ok(path.to_owned());
    }
    if !input.is_empty() && input.bytes().all(|byte| byte.is_ascii_digit()) {
        for library in libraries {
            let candidate = steamapps(library)
                .join("workshop/content/431960")
                .join(input);
            if candidate.is_dir() {
                return Ok(candidate);
            }
        }
        bail!(
            "Workshop item {input} is not installed in the searched Steam libraries. Download it with Steam first, or pass --steam-library PATH or a project directory."
        );
    }
    bail!("wallpaper directory does not exist: {input}")
}

pub fn shared_assets(libraries: &[PathBuf]) -> Option<PathBuf> {
    libraries
        .iter()
        .map(|root| steamapps(root).join("common/wallpaper_engine/assets"))
        .find(|path| path.is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn resolves_workshop_and_shared_assets_in_macos_steam() -> Result<()> {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let home =
            std::env::temp_dir().join(format!("rxwpe-steam-{}-{unique}", std::process::id()));
        let root = home.join("Library/Application Support/Steam");
        let wallpaper = root.join("steamapps/workshop/content/431960/123456");
        let assets = root.join("steamapps/common/wallpaper_engine/assets");
        std::fs::create_dir_all(&wallpaper)?;
        std::fs::create_dir_all(&assets)?;
        let libraries = vec![root.clone()];
        let resolved = resolve("123456", &libraries);
        let found_assets = shared_assets(&libraries);
        let direct_steamapps = resolve("123456", &[root.join("steamapps")]);
        std::fs::remove_dir_all(&home)?;
        assert_eq!(resolved?, wallpaper);
        assert_eq!(direct_steamapps?, wallpaper);
        assert_eq!(found_assets, Some(assets));
        Ok(())
    }
}
