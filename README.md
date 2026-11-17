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
cargo run -- examples/static-scene --window
```

Press Escape or close the window to stop.

You can also build and run with Nix:

```sh
nix build
nix run . -- examples/static-scene --window
```

## Play a wallpaper

Use the directory containing the wallpaper's `project.json`:

```sh
cargo run -- /path/to/wallpaper
```

For an already-downloaded Steam Workshop wallpaper, use its ID:

```sh
cargo run -- 123456789
```

If it is in another Steam library, specify that library:

```sh
cargo run -- 123456789 --steam-library /path/to/SteamLibrary
```

rxwpe does not download wallpapers. Download them through Steam first.

If a wallpaper needs Wallpaper Engine's shared assets, provide their directory:

```sh
cargo run -- /path/to/wallpaper --assets /path/to/wallpaper_engine/assets
```

## Set the desktop wallpaper

Playback automatically uses your desktop on macOS or Linux with a Wayland compositor that supports layer-shell:

```sh
cargo run -- /path/to/wallpaper
```

To choose a display on macOS, use its name or index, starting at zero:

```sh
cargo run -- /path/to/wallpaper --display 0
```

On Wayland, choose a display by name:

```sh
cargo run -- /path/to/wallpaper --display DP-1
```

Each process displays one wallpaper on one display. Press Ctrl+C in the terminal to stop.

On macOS, playback stops if the selected display disconnects.

## Adjust playback

Choose how the wallpaper fits the display and set the maximum frame rate:

```sh
cargo run -- /path/to/wallpaper --scale fit --fps 30
```

`--scale` accepts `fit`, `fill`, or `stretch`. The default is `fill`.

Set the initial window size:

```sh
cargo run -- /path/to/wallpaper --window --size 1280x720
```

Override an option defined by the wallpaper:

```sh
cargo run -- examples/static-scene --property opacity=0.5
```

Unsupported scene behavior prevents playback by default. To preview the supported parts:

```sh
cargo run -- /path/to/wallpaper --allow-partial
```

## Declarative configuration

Import `inputs.rxwpe.homeManagerModules.default` in your Home Manager configuration. It works with standalone Home Manager and Home Manager inside NixOS or nix-darwin.

```nix
{
  imports = [ inputs.rxwpe.homeManagerModules.default ];

  services.rxwpe = {
    enable = true;
    settings = {
      wallpaper = "123456789";
      fps = 60;
    };
  };
}
```

Leave `display` unset to use the primary display on macOS or let the compositor choose on Wayland. To select a display, set `display = "DP-1"` on Wayland or use a display name or index such as `display = "0"` on macOS.

Leave `fps` and `scale` unset, or set them to `null`, to use the command-line defaults.

`settings.wallpaper` accepts a directory or a Workshop ID. You can also set `assets`, `steamLibraries`, `size`, `scale`, `properties`, `allowPartial`, and `extraArgs` inside `settings`. The wallpaper starts with your graphical session. Set `services.rxwpe.enable = false` to disable it.

## Command help

```sh
cargo run -- --help
```
