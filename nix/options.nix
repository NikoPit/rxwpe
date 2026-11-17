{
  lib,
  pkgs,
  config,
}:

let
  settingsOptions = {
    wallpaper = lib.mkOption {
      type = lib.types.oneOf [
        lib.types.path
        lib.types.str
      ];
      description = "A wallpaper directory or an installed Workshop ID.";
    };
    display = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "DP-1";
      description = ''
        A display name, or a zero-based index on macOS.
        Null uses the primary display on macOS and lets the compositor choose on Wayland.
      '';
    };
    assets = lib.mkOption {
      type = lib.types.nullOr (
        lib.types.oneOf [
          lib.types.path
          lib.types.str
        ]
      );
      default = null;
      description = "The Wallpaper Engine shared assets directory.";
    };
    steamLibraries = lib.mkOption {
      type = lib.types.listOf (
        lib.types.oneOf [
          lib.types.path
          lib.types.str
        ]
      );
      default = [ ];
      description = "Additional Steam library directories to search.";
    };
    fps = lib.mkOption {
      type = lib.types.nullOr (lib.types.ints.between 1 240);
      default = null;
      description = "The maximum playback frame rate. Null uses the CLI default.";
    };
    scale = lib.mkOption {
      type = lib.types.nullOr (
        lib.types.enum [
          "fit"
          "fill"
          "stretch"
        ]
      );
      default = null;
      description = "How to fit the wallpaper to the output. Null uses the CLI default.";
    };
    size = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "1920x1080";
      description = "The window size, in WIDTHxHEIGHT form.";
    };
    allowPartial = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Render supported parts when the wallpaper has unsupported behavior.";
    };
    properties = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { };
      description = "Wallpaper property overrides passed as NAME=VALUE.";
    };
    extraArgs = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      description = "Additional arguments passed to rxwpe.";
    };
  };
in
{
  enable = lib.mkEnableOption "rxwpe wallpaper playback";

  package = lib.mkOption {
    type = lib.types.package;
    default = pkgs.callPackage ./package.nix { };
    defaultText = lib.literalExpression "pkgs.callPackage ./nix/package.nix { }";
    description = "The rxwpe package used to play wallpapers.";
  };

  settings = lib.mkOption {
    type = lib.types.submodule { options = settingsOptions; };
    default = { };
    description = "Wallpaper playback settings.";
    example = {
      wallpaper = "/home/alice/wallpapers/main";
      fps = 60;
    };
  };

  systemdTarget = lib.mkOption {
    type = lib.types.str;
    default = config.wayland.systemd.target;
    defaultText = lib.literalExpression "config.wayland.systemd.target";
    example = "sway-session.target";
    description = "The systemd user target that starts and stops wallpapers on Linux.";
  };
}
