use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use anyhow::{Context, Result};

static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

/// A private, seekable copy of a packaged media asset. Removed on drop.
pub(crate) struct MediaFile(PathBuf);

impl MediaFile {
    pub(crate) fn from_bytes(bytes: &[u8]) -> Result<Self> {
        loop {
            let path = std::env::temp_dir().join(format!(
                "rxwpe-media-{}-{}",
                std::process::id(),
                NEXT_FILE.fetch_add(1, Ordering::Relaxed)
            ));
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file: File = match options.open(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error).context("create temporary media file"),
            };
            let owned = Self(path);
            file.write_all(bytes)
                .context("write temporary media file")?;
            return Ok(owned);
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for MediaFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_assets_and_removes_them_when_closed() -> Result<()> {
        let file = MediaFile::from_bytes(b"packed media")?;
        let path = file.path().to_owned();
        assert_eq!(std::fs::read(&path)?, b"packed media");
        drop(file);
        assert!(!path.exists());
        Ok(())
    }
}
