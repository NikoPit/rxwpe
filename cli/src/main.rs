mod workshop;

use std::{path::PathBuf, time::Duration};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use rxwpe_gpu::{Frame, Gpu};
use rxwpe_output::{OutputOptions, ScaleMode};
use rxwpe_parser::{Project, WallpaperType};
use rxwpe_scene::{SceneOptions, ScenePlayer};
use rxwpe_video::VideoPlayer;

#[derive(Parser)]
#[command(
    name = "rxwpe",
    version,
    about = "Play Wallpaper Engine wallpapers",
    disable_help_subcommand = true
)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Args)]
struct ProjectArgs {
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
}

impl ProjectArgs {
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

#[derive(Subcommand)]
enum Command {
    /// Play a wallpaper on the desktop.
    Play {
        #[command(flatten)]
        project: ProjectArgs,
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
    },
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
    match Args::parse().command {
        Command::Play {
            project,
            window,
            size,
            fps,
            scale,
            display,
            allow_partial,
        } => {
            let project = project.load()?;
            // Initialize a player lazily using the device selected for the output surface.
            let mut player = None;
            let options = OutputOptions {
                window,
                width: size.0,
                height: size.1,
                fps,
                title: format!("rxwpe: {}", project.metadata.title),
                scale: match scale {
                    Scaling::Fit => ScaleMode::Fit,
                    Scaling::Fill => ScaleMode::Fill,
                    Scaling::Stretch => ScaleMode::Stretch,
                },
                display,
            };
            rxwpe_output::run(options, move |gpu, elapsed, _size| {
                if player.is_none() {
                    player = Some(Player::open(&project, gpu, allow_partial)?);
                }
                player
                    .as_mut()
                    .context("player was not initialized")?
                    .render(gpu, Duration::from_secs_f64(elapsed))
            })?;
        }
    }
    Ok(())
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
        let args = Args::try_parse_from(["rxwpe", "play", "wallpaper"]).unwrap();
        assert!(matches!(args.command, Command::Play { window: false, .. }));
        let args = Args::try_parse_from(["rxwpe", "play", "wallpaper", "--window"]).unwrap();
        assert!(matches!(args.command, Command::Play { window: true, .. }));
        assert!(
            Args::try_parse_from(["rxwpe", "play", "wallpaper", "--output", "wayland"]).is_err()
        );
    }

    #[test]
    fn play_is_the_only_command() {
        let mut command = <Args as clap::CommandFactory>::command();
        command.build();
        let commands: Vec<_> = command
            .get_subcommands()
            .map(|cmd| cmd.get_name())
            .collect();
        assert_eq!(commands, ["play"]);
        for removed in ["inspect", "screenshot", "help"] {
            assert!(Args::try_parse_from(["rxwpe", removed]).is_err());
        }
    }
}
