# Usage

```sh
rxwpe [OPTIONS] <WALLPAPER>
```

Desktop playback supports macOS and Linux with a Wayland compositor that supports layer-shell.

## Choose a wallpaper

Pass the directory containing `project.json`:

```sh
rxwpe /path/to/wallpaper
```

For an already-downloaded Steam Workshop wallpaper, pass its ID:

```sh
rxwpe 123456789
```

To find the ID, open the wallpaper's page in the Steam Workshop. In Steam, use the page's Share button to copy its link, or open the page in a browser. The number after `?id=` is the wallpaper ID:

```text
https://steamcommunity.com/sharedfiles/filedetails/?id=123456789
```

This wallpaper's ID is `123456789`.

rxwpe does not download wallpapers. Download Workshop wallpapers through Steam first.

Specify additional Steam libraries with `--steam-library`. This option can be repeated:

```sh
rxwpe 123456789 --steam-library /path/to/SteamLibrary
```

rxwpe automatically finds Wallpaper Engine's shared assets in standard Steam libraries. If you want to specifiy your own assets, you can do it like this:

```sh
rxwpe /path/to/wallpaper --assets /path/to/wallpaper_engine/assets
```

## macOS wallpaper setup

Wallpaper Engine has no official macOS version, so you need to download the wallpaper and assets then transfer them to your macOS machine for it to work.

1. On the Windows or Linux machine, install Wallpaper Engine through Steam and wait for the wallpaper to finish downloading.
2. Find its directory in the Steam library containing the wallpaper:

   ```text
   <SteamLibrary>/steamapps/workshop/content/431960/<ID>/
   ```

   You can find Steam library locations in Steam Settings > Storage. Copy the entire directory to your Mac.

3. Copy wallpaper engine's assets directory. In Steam, right-click Wallpaper Engine and choose Manage > Browse local files. Copy the entire `assets` directory to your Mac. Its usual source location is:

   ```text
   <SteamLibrary>/steamapps/common/wallpaper_engine/assets/
   ```

4. Play the copied wallpaper using its directory path:

   ```sh
   rxwpe <WALLPAPER_DIR> --assets <ASSETS_DIR>
   ```

Steam does not need to be installed on your Mac for playback by directory path. Use the full path for files copied outside a Steam library; the Workshop ID alone will not find them.

For Home Manager, set `settings.wallpaper` and `settings.assets` to the corresponding absolute paths on your Mac. See [Nix configuration](nix.md) for an example using local files.

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

To inspect all of the properties in a wallpaper, use its directory or numeric Workshop ID:

```sh
rxwpe /path/to/wallpaper --list-properties
rxwpe 123456789 --list-properties
```

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
