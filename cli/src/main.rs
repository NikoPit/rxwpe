mod workshop;

use std::{path::PathBuf, time::Duration};

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use rxwpe_gpu::{Frame, Gpu};
use rxwpe_output::{OutputOptions, ScaleMode};
use rxwpe_parser::{Project, WallpaperType};
use rxwpe_scene::{SceneOptions, ScenePlayer};
use rxwpe_video::VideoPlayer;

#[derive(Parser)]
#[command(name = "rxwpe", version, about = "Play Wallpaper Engine wallpapers")]
struct Args {
    /// Local project directory or an already-downloaded Workshop ID.
    wallpaper: String,
    /// Directory containing steamapps. May be repeated for additional libraries.
    #[arg(long)]
    steam_library: Vec<PathBuf>,
    /// Official Wallpaper Engine shared assets directory.
    #[arg(long)]
    assets: Option<PathBuf>,
    /// Override a wallpaper property. Value is JSON, or a plain string.
    #[arg(long = "property", value_name = "NAME=VALUE")]
    properties: Vec<String>,
    /// Preview in a normal window instead of setting the desktop wallpaper.
    #[arg(long)]
    window: bool,
    #[arg(long, default_value = "1280x720", value_parser = parse_size)]
    size: (u32, u32),
    #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u32).range(1..=240))]
    fps: u32,
    #[arg(long, value_enum, default_value = "fill")]
    scale: Scaling,
    /// Display name, or a zero-based index on macOS. Defaults to the primary on macOS, compositor choice on Wayland.
    #[arg(long)]
    display: Option<String>,
    /// Preview supported scene layers while omitting unsupported behavior.
    #[arg(long)]
    allow_partial: bool,
}

impl Args {
    fn load(&self) -> Result<Project> {
        let libraries = workshop::libraries(&self.steam_library);
        let directory = workshop::resolve(&self.wallpaper, &libraries)?;
        let shared = self
            .assets
            .clone()
            .or_else(|| workshop::shared_assets(&libraries));
        let mut project = Project::load(directory, shared.as_deref())?;
        for assignment in &self.properties {
            let (name, raw) = assignment
                .split_once('=')
                .context("property must be NAME=VALUE")?;
            let value = serde_json::from_str(raw).unwrap_or_else(|_| raw.into());
            project.set_property(name, value)?;
        }
        Ok(project)
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum Scaling {
    Fit,
    Fill,
    Stretch,
}

enum Player {
    Scene(ScenePlayer),
    Video(VideoPlayer),
}

impl Player {
    fn open(project: &Project, gpu: &Gpu, allow_partial: bool) -> Result<Self> {
        match project.metadata.kind {
            WallpaperType::Scene => {
                let player = ScenePlayer::open(project, gpu, SceneOptions { allow_partial })?;
                for warning in player.warnings() {
                    eprintln!("warning: {warning}");
                }
                Ok(Self::Scene(player))
            }
            WallpaperType::Video => {
                eprintln!("Video audio is not implemented yet; playback is muted.");
                Ok(Self::Video(VideoPlayer::open(project.entry_path()?)?))
            }
            WallpaperType::Web => {
                rxwpe_web::WebPlayer::open(project)?;
                unreachable!(
                    "web player admission always returns an error until a browser backend exists"
                )
            }
        }
    }

    fn render(&mut self, gpu: &Gpu, elapsed: Duration) -> Result<Frame> {
        match self {
            Self::Scene(player) => player.render(gpu, elapsed),
            Self::Video(player) => player.render(gpu, elapsed),
        }
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = Args::parse();
    let project = args.load()?;
    // Initialize a player lazily using the device selected for the output surface.
    let mut player = None;
    let options = OutputOptions {
        window: args.window,
        width: args.size.0,
        height: args.size.1,
        fps: args.fps,
        title: format!("rxwpe: {}", project.metadata.title),
        scale: match args.scale {
            Scaling::Fit => ScaleMode::Fit,
            Scaling::Fill => ScaleMode::Fill,
            Scaling::Stretch => ScaleMode::Stretch,
        },
        display: args.display,
    };
    rxwpe_output::run(options, move |gpu, elapsed, _size| {
        if player.is_none() {
            player = Some(Player::open(&project, gpu, args.allow_partial)?);
        }
        player
            .as_mut()
            .context("player was not initialized")?
            .render(gpu, Duration::from_secs_f64(elapsed))
    })
}

fn parse_size(input: &str) -> Result<(u32, u32), String> {
    let (width, height) = input.split_once('x').ok_or("expected WIDTHxHEIGHT")?;
    let width = width.parse::<u32>().map_err(|_| "invalid width")?;
    let height = height.parse::<u32>().map_err(|_| "invalid height")?;
    if width == 0 || height == 0 || width > 16_384 || height > 16_384 {
        return Err("dimensions must be between 1 and 16384".into());
    }
    Ok((width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_defaults_to_desktop_and_allows_window_preview() {
        let args = Args::try_parse_from(["rxwpe", "wallpaper"]).unwrap();
        assert_eq!(args.wallpaper, "wallpaper");
        assert!(!args.window);
        let args = Args::try_parse_from(["rxwpe", "wallpaper", "--window"]).unwrap();
        assert!(args.window);
        assert!(Args::try_parse_from(["rxwpe", "wallpaper", "--output", "wayland"]).is_err());
    }

    #[test]
    fn cli_has_no_subcommands() {
        let command = <Args as clap::CommandFactory>::command();
        assert!(!command.has_subcommands());
        for removed in ["play", "inspect", "screenshot", "help"] {
            assert!(Args::try_parse_from(["rxwpe", removed, "wallpaper"]).is_err());
        }
    }
}
