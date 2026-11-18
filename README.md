# rxwpe

Play Wallpaper Engine wallpapers on Linux and macOS.

Currently supports basic image scenes and muted video playback. Web wallpapers and advanced scene effects are not supported yet.

## Supported platforms

- MacOS
- Linux (Wayland)

## Get started

Install Rust and FFmpeg, then run this from the repository directory to install rxwpe:

```sh
cargo install --path cli --locked
```

Play a wallpaper with its Steam Workshop ID:

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
{
  imports = [ inputs.rxwpe.homeManagerModules.default ];

  services.rxwpe = {
    enable = true;
    settings.wallpaper = "123456789";
  };
}
```

See [Nix configuration](docs/nix.md) for setup and options.
