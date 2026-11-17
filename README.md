# rxwpe

Play Wallpaper Engine wallpapers on Linux and macOS.

## Get started

Install Rust and FFmpeg, then run this from the repository directory:

```sh
cargo install --path cli --locked
```

Play an already-downloaded Steam Workshop wallpaper using its ID:

```sh
rxwpe 123456789
```

Press Ctrl+C to stop. See [usage](docs/usage.md) for more options.

## Declarative configuration with Nix

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
