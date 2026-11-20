//! Wallpaper selection and playback on the display's GPU.

use std::time::Duration;

use crate::project::{Project, WallpaperType};
use crate::render::{Frame, Gpu};
use anyhow::{Result, bail};

mod audio;
mod decoder;
mod file;
mod particles;
mod scene;
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "Pause controls are not exposed by the CLI yet.")
)]
mod video;

use scene::ScenePlayer;
use video::VideoPlayer;

pub(crate) enum Player {
    Scene(ScenePlayer),
    Video(VideoPlayer),
}

impl Player {
    pub(crate) fn open(project: &Project, gpu: &Gpu, allow_partial: bool) -> Result<Self> {
        match project.metadata.kind {
            WallpaperType::Scene => {
                let player = ScenePlayer::open(project, gpu, allow_partial)?;
                for warning in player.warnings() {
                    eprintln!("warning: {warning}");
                }
                Ok(Self::Scene(player))
            }
            WallpaperType::Video => {
                eprintln!("Video audio is not implemented yet; playback is muted.");
                Ok(Self::Video(VideoPlayer::open(project.entry_path()?)?))
            }
            WallpaperType::Web => bail!(
                "web wallpaper playback is not implemented yet: {}",
                project.metadata.file
            ),
        }
    }

    pub(crate) fn render(&mut self, gpu: &Gpu, elapsed: Duration) -> Result<Frame> {
        match self {
            Self::Scene(player) => player.render(gpu, elapsed),
            Self::Video(player) => player.render(gpu, elapsed),
        }
    }
}
