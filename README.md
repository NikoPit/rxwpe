# rxwpe

Play Wallpaper Engine wallpapers on Linux and macOS.

## Supported platforms

- macOS
- Linux with a Wayland compositor that supports layer-shell

## Get started

Install Rust and FFmpeg with `ffplay`, then run this from the repository directory to install rxwpe:

```sh
cargo install --path . --locked
```

On macOS, follow [macOS wallpaper setup](docs/usage.md#macos-wallpaper-setup) to prepare your files and play them by directory path.

On Linux, make sure you have wallpaper engine installed on this machine, and all of the assets have finished downloading. Although wallpaper engine does not support linux, installing it still also downloads all of the workshop items with it, which is needed for rxwpe.

Play the downloaded wallpaper using its Workshop ID:

```sh
rxwpe 123456789
```

To get the ID of a wallpaper, open the wallpaper's Steam Workshop page and copy the number after `?id=` in its URL. For example, `https://steamcommunity.com/sharedfiles/filedetails/?id=123456789` has the ID `123456789`.

See [usage](docs/usage.md) for more options.

## Declarative configuration with Nix

Add rxwpe to your `flake.nix` inputs:

```nix
inputs.rxwpe.url = "github:NikoPit/rxwpe";
```

Then import its Home Manager module and choose a wallpaper:

```nix
{ inputs, ... }:

{
  imports = [ inputs.rxwpe.homeManagerModules.default ];

  services.rxwpe = {
    enable = true;
    settings.wallpaper = "123456789";
  };
}
```

See [Nix configuration](docs/nix.md) for setup and options.
