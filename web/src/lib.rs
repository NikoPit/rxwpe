//! Admission for web projects. Browser playback is not implemented yet.
//!
//! A future browser host belongs here, including Wallpaper Engine's JavaScript
//! property/audio bridge and a presentation path compatible with `output`.

use anyhow::{Result, bail, ensure};
use rxwpe_parser::{Project, WallpaperType};

pub struct WebPlayer;

impl WebPlayer {
    pub fn open(project: &Project) -> Result<Self> {
        ensure!(
            project.metadata.kind == WallpaperType::Web,
            "expected a web wallpaper"
        );
        bail!(
            "web wallpaper playback is not implemented yet: {}",
            project.metadata.file
        )
    }
}
