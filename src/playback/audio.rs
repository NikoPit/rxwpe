use std::{
    path::Path,
    process::{Child, Command, Stdio},
};

use anyhow::{Context, Result, ensure};

use super::file::MediaFile;

/// An FFplay process owned by the wallpaper instance.
pub(crate) struct AudioPlayback {
    child: Child,
    _file: MediaFile,
}

impl AudioPlayback {
    pub(crate) fn open(bytes: &[u8], looped: bool, volume: f32) -> Result<Self> {
        Self::open_with(Path::new("ffplay"), bytes, looped, volume)
    }

    fn open_with(executable: &Path, bytes: &[u8], looped: bool, volume: f32) -> Result<Self> {
        ensure!(
            volume.is_finite() && (0. ..=1.).contains(&volume),
            "audio volume must be between zero and one"
        );
        let file = MediaFile::from_bytes(bytes)?;
        let mut command = Command::new(executable);
        command
            .args(["-nodisp", "-autoexit", "-loglevel", "error", "-volume"])
            .arg((volume * 100.).round().to_string());
        if looped {
            command.args(["-loop", "0"]);
        }
        let child = command
            .arg(file.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| {
                format!(
                    "cannot run {} to play wallpaper sound",
                    executable.display()
                )
            })?;
        Ok(Self { child, _file: file })
    }
}

impl Drop for AudioPlayback {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        thread,
        time::Duration,
    };

    #[cfg(unix)]
    #[test]
    fn owns_player_lifecycle_and_packed_asset() -> Result<()> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "rxwpe-ffplay-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let log = path.with_extension("log");
        fs::write(
            &path,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nsleep 30\n",
                log.display()
            ),
        )?;
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
        let player = AudioPlayback::open_with(&path, b"audio", true, 0.5)?;
        for _ in 0..50 {
            if log.exists() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let args = fs::read_to_string(&log)?;
        assert!(args.contains("-loop\n0\n"));
        assert!(args.contains("50\n"));
        let media_path = args.lines().last().unwrap().to_owned();
        assert_eq!(fs::read(&media_path)?, b"audio");
        drop(player);
        assert!(!PathBuf::from(media_path).exists());
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(log);
        Ok(())
    }
}
