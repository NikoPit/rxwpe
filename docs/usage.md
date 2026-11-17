# Usage

```sh
rxwpe [OPTIONS] <WALLPAPER>
```

Desktop playback supports macOS and Linux with a Wayland compositor that supports layer-shell. Basic image scenes and muted video playback are supported. Web wallpapers and advanced scene effects are not supported yet.

## Choose a wallpaper

Pass the directory containing `project.json`:

```sh
rxwpe /path/to/wallpaper
```

For an already-downloaded Steam Workshop wallpaper, pass its ID:

```sh
rxwpe 123456789
```

rxwpe does not download wallpapers. Download Workshop wallpapers through Steam first.

Specify additional Steam libraries with `--steam-library`. This option can be repeated:

```sh
rxwpe 123456789 --steam-library /path/to/SteamLibrary
```

If a wallpaper needs Wallpaper Engine's shared assets, provide their directory:

```sh
rxwpe /path/to/wallpaper --assets /path/to/wallpaper_engine/assets
```

## Choose a display

Without `--display`, macOS uses the primary display and Wayland lets the compositor choose. Each process displays one wallpaper on one display.

On Wayland, pass a display name:

```sh
rxwpe /path/to/wallpaper --display DP-1
```

On macOS, pass a display name or index, starting at zero:

```sh
rxwpe /path/to/wallpaper --display 0
```

On macOS, playback stops if the selected display disconnects. Press Ctrl+C to stop desktop playback manually.

## Preview in a window

```sh
rxwpe /path/to/wallpaper --window --size 1280x720
```

Press Escape or close the window to stop. `--size` sets the initial window size and defaults to `1280x720`.

## Adjust playback

```sh
rxwpe /path/to/wallpaper --scale fit --fps 60
```

| Option | Accepted values | Default |
|---|---|---|
| `--fps` | An integer between 1 and 240 | `30` |
| `--scale` | `fit`, `fill`, `stretch` | `fill` |

`fit` keeps the entire image visible and may leave borders. `fill` fills the display and may crop the image. `stretch` fills the display without keeping the image's aspect ratio.

Override a property defined by the wallpaper with `--property NAME=VALUE`. Values accept JSON or plain strings. Repeat the option for multiple properties:

```sh
rxwpe /path/to/wallpaper --property opacity=0.5
```

Unsupported scene behavior prevents playback by default. To preview supported parts while omitting unsupported behavior:

```sh
rxwpe /path/to/wallpaper --allow-partial
```

## Help and version

```sh
rxwpe --help
rxwpe --version
```
