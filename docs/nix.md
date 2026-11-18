# Nix configuration

The flake provides packages and a Home Manager module for Linux and Apple Silicon macOS. The module works with standalone Home Manager and Home Manager inside NixOS or nix-darwin.

## Home Manager setup

Add rxwpe to the `inputs` in your `flake.nix`:

```nix
inputs.rxwpe.url = "github:NikoPit/rxwpe";
```

Include `inputs` in your flake's outputs arguments, for example `outputs = inputs@{ self, nixpkgs, ... }: ...`.

Then import its module in your Home Manager configuration:

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

Make `inputs` available to your Home Manager modules through `extraSpecialArgs`. For example, when creating a standalone Home Manager configuration:

```nix
home-manager.lib.homeManagerConfiguration {
  inherit pkgs;
  extraSpecialArgs = { inherit inputs; };
  modules = [ ./home.nix ];
}
```

Apply your Home Manager configuration to install rxwpe and start the wallpaper with your graphical session. Set `services.rxwpe.enable = false` and apply again to disable it.

## Wallpaper settings

Configure one wallpaper through `services.rxwpe.settings`. `wallpaper` is required when the service is enabled. All other settings are optional.

| Setting | Default | Usage |
|---|---|---|
| `wallpaper` | Required | A wallpaper directory or an already-downloaded Workshop ID as a string. |
| `assets` | `null` | The shared Wallpaper Engine assets directory. When unset, rxwpe searches Steam installations. |
| `steamLibraries` | `[ ]` | Additional Steam library directories containing `steamapps`. |
| `display` | `null` | A display name, or a zero-based index on macOS as a string. When unset, macOS uses the primary display and Wayland lets the compositor choose. |
| `fps` | `null` | An integer between 1 and 240. When unset, the CLI default is `30`. |
| `scale` | `null` | `"fit"`, `"fill"`, or `"stretch"`. When unset, the CLI default is `"fill"`. |
| `size` | `null` | An initial window size such as `"1280x720"`. When unset, the CLI default is `"1280x720"`. |
| `allowPartial` | `false` | Render supported parts while omitting unsupported scene behavior. |
| `properties` | `{ }` | Wallpaper property overrides. Values are strings containing JSON or plain text. |
| `extraArgs` | `[ ]` | Additional CLI arguments as a list of strings. |

Settings with a `null` value do not pass a corresponding CLI argument. See [usage](usage.md) for playback options.

A local wallpaper and an explicit assets directory work without Steam installed:

```nix
services.rxwpe = {
  enable = true;
  settings = {
    wallpaper = "/home/alice/wallpapers/main";
    assets = "/home/alice/wallpaper-assets";
  };
};
```

To select a Workshop wallpaper and customize playback:

```nix
services.rxwpe = {
  enable = true;
  settings = {
    wallpaper = "123456789";
    steamLibraries = [ "/mnt/games/SteamLibrary" ];
    display = "DP-1";
    fps = 60;
    scale = "fit";
    properties.opacity = "0.5";
  };
};
```

On macOS, use a display name or an index such as `display = "0"`. Download Workshop wallpapers through Steam before using their IDs.

## Service options

These options belong directly under `services.rxwpe`:

| Option | Default | Usage |
|---|---|---|
| `enable` | `false` | Enable wallpaper playback in your graphical session. |
| `package` | The package built by this module | Override the rxwpe package. |
| `systemdTarget` | `config.wayland.systemd.target` | Choose the Linux session target that starts and stops the service, such as `"sway-session.target"`. |

On Linux, the user service is named `rxwpe` and requires `WAYLAND_DISPLAY` in the systemd user environment. To check its status and logs:

```sh
systemctl --user status rxwpe
journalctl --user -u rxwpe
```

On macOS, Home Manager starts rxwpe as a launchd agent in your graphical session.
