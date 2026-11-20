use std::{path::PathBuf, time::Duration};

use crate::display::{OutputOptions, ScaleMode};
use crate::playback::Player;
use crate::project::Project;
use anyhow::{Context, Result};
use clap::Parser;

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
    scale: ScaleMode,
    /// Display name, or a zero-based index on macOS. Defaults to the primary on macOS, compositor choice on Wayland.
    #[arg(long)]
    display: Option<String>,
    /// Preview supported scene layers while omitting unsupported behavior.
    #[arg(long)]
    allow_partial: bool,
}

impl Args {
    fn load(&self) -> Result<Project> {
        let mut project =
            Project::resolve(&self.wallpaper, &self.steam_library, self.assets.as_deref())?;
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

pub(crate) fn run() -> Result<()> {
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
        scale: args.scale,
        display: args.display,
    };
    crate::display::run(options, move |gpu, elapsed, _size| {
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
    fn dimensions_and_frame_rates_enforce_their_bounds() {
        for invalid in [
            "0x720",
            "1280x0",
            "16385x1",
            "1x16385",
            "4294967296x1",
            "-1x720",
            "1280",
            "1280X720",
            "1280x720x1",
            "axb",
        ] {
            assert!(
                Args::try_parse_from(["rxwpe", "wallpaper", "--size", invalid]).is_err(),
                "accepted invalid size {invalid}"
            );
        }
        for valid in ["1x1", "16384x16384", "1920x1080"] {
            assert!(Args::try_parse_from(["rxwpe", "wallpaper", "--size", valid]).is_ok());
        }
        for invalid in ["0", "241", "-1", "1.5", "abc"] {
            assert!(
                Args::try_parse_from(["rxwpe", "wallpaper", "--fps", invalid]).is_err(),
                "accepted invalid frame rate {invalid}"
            );
        }
        for valid in ["1", "240"] {
            assert!(Args::try_parse_from(["rxwpe", "wallpaper", "--fps", valid]).is_ok());
        }
    }

    #[test]
    fn playback_defaults_to_desktop_and_allows_window_preview() {
        let args = Args::try_parse_from(["rxwpe", "wallpaper"]).unwrap();
        assert_eq!(args.wallpaper, "wallpaper");
        assert!(!args.window);
        assert!(matches!(args.scale, ScaleMode::Fill));
        let args = Args::try_parse_from(["rxwpe", "wallpaper", "--window"]).unwrap();
        assert!(args.window);
        for scale in ["fit", "fill", "stretch"] {
            let args = Args::try_parse_from(["rxwpe", "wallpaper", "--scale", scale]).unwrap();
            assert!(matches!(
                (scale, args.scale),
                ("fit", ScaleMode::Fit)
                    | ("fill", ScaleMode::Fill)
                    | ("stretch", ScaleMode::Stretch)
            ));
        }
        assert!(Args::try_parse_from(["rxwpe", "wallpaper", "--scale", "invalid"]).is_err());
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
