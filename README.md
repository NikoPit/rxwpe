# rxwpe

Play Wallpaper Engine wallpapers on Linux and macOS.

Currently supports basic image scenes and muted video playback. Web wallpapers and advanced scene effects are not supported yet.

## Get started

On Linux or Apple Silicon macOS with Nix installed, enter the development environment:

```sh
nix develop
```

Without Nix, install Rust and FFmpeg before running the commands below.

Try the included example in a window:

```sh
cargo run -- play examples/static-scene --window
```

Press Escape or close the window to stop.

You can also build and run with Nix:

```sh
nix build
nix run . -- play examples/static-scene --window
```

## Play a wallpaper

Use the directory containing the wallpaper's `project.json`:

```sh
cargo run -- play /path/to/wallpaper
```

For an already-downloaded Steam Workshop wallpaper, use its ID:

```sh
cargo run -- play 123456789
```

If it is in another Steam library, specify that library:

```sh
cargo run -- play 123456789 --steam-library /path/to/SteamLibrary
```

rxwpe does not download wallpapers. Download them through Steam first.

If a wallpaper needs Wallpaper Engine's shared assets, provide their directory:

```sh
cargo run -- play /path/to/wallpaper --assets /path/to/wallpaper_engine/assets
```

## Set the desktop wallpaper

Playback automatically uses your desktop on macOS or Linux with a Wayland compositor that supports layer-shell:

```sh
cargo run -- play /path/to/wallpaper
```

To choose a display on macOS, use its name or index, starting at zero:

```sh
cargo run -- play /path/to/wallpaper --display 0
```

On Wayland, choose a display by name:

```sh
cargo run -- play /path/to/wallpaper --display DP-1
```

Each process displays one wallpaper on one display. Press Ctrl+C in the terminal to stop.

On macOS, playback stops if the selected display disconnects.

## Adjust playback

Choose how the wallpaper fits the display and set the maximum frame rate:

```sh
cargo run -- play /path/to/wallpaper --scale fit --fps 30
```

`--scale` accepts `fit`, `fill`, or `stretch`. The default is `fill`.

Set the initial window size:

```sh
cargo run -- play /path/to/wallpaper --window --size 1280x720
```

Override an option defined by the wallpaper:

```sh
cargo run -- play examples/static-scene --property opacity=0.5
```

Unsupported scene behavior prevents playback by default. To preview the supported parts:

```sh
cargo run -- play /path/to/wallpaper --allow-partial
```

## Command help

```sh
cargo run -- --help
cargo run -- play --help
```
