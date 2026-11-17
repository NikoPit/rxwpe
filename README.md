# rxwpe

Play Wallpaper Engine wallpapers on Linux.

Currently supports basic image scenes and muted video playback. Web wallpapers and advanced scene effects are not supported yet.

## Get started

With Nix installed, enter the development environment:

```sh
nix develop
```

Try the included example in a window:

```sh
cargo run -- play examples/static-scene
```

Press Escape or close the window to stop.

You can also build and run with Nix:

```sh
nix build
nix run . -- play examples/static-scene
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

On a Wayland desktop that supports layer-shell:

```sh
cargo run -- play /path/to/wallpaper --output wayland
```

To choose a display:

```sh
cargo run -- play /path/to/wallpaper --output wayland --display DP-1
```

Each process displays one wallpaper on one display. Press Ctrl+C in the terminal to stop.

## Adjust playback

Choose how the wallpaper fits the display and set the maximum frame rate:

```sh
cargo run -- play /path/to/wallpaper --scale fit --fps 30
```

`--scale` accepts `fit`, `fill`, or `stretch`. The default is `fill`.

Set the initial window size:

```sh
cargo run -- play /path/to/wallpaper --size 1280x720
```

Override an option defined by the wallpaper:

```sh
cargo run -- play examples/static-scene --property opacity=0.5
```

Inspect a wallpaper to see its available options:

```sh
cargo run -- inspect /path/to/wallpaper
```

Unsupported scene behavior prevents playback by default. To preview the supported parts:

```sh
cargo run -- play /path/to/wallpaper --allow-partial
```

## Save a screenshot

```sh
cargo run -- screenshot /path/to/wallpaper --output wallpaper.png
```

Choose a position in a video, in seconds:

```sh
cargo run -- screenshot /path/to/video-wallpaper --time 3.5 --output wallpaper.png
```

Screenshots do not change your desktop wallpaper.

## Command help

```sh
cargo run -- --help
cargo run -- play --help
```
