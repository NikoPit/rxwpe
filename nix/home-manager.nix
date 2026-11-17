{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.rxwpe;
  # ExecStart is parsed by systemd, including its own percent and dollar expansion.
  escapeExecArgs = lib.concatMapStringsSep " " (
    arg: lib.replaceStrings [ "%" "$" ] [ "%%" "$$" ] (builtins.toJSON arg)
  );
  args =
    (import ./args.nix {
      inherit lib;
      inherit (cfg) package;
    })
      cfg.settings;
in
{
  options.services.rxwpe = import ./options.nix { inherit lib pkgs config; };

  config = lib.mkIf cfg.enable {
    home.packages = [ cfg.package ];

    systemd.user.services.rxwpe = lib.mkIf pkgs.stdenv.hostPlatform.isLinux {
      Unit = {
        Description = "rxwpe wallpaper";
        After = [ cfg.systemdTarget ];
        PartOf = [ cfg.systemdTarget ];
        ConditionEnvironment = "WAYLAND_DISPLAY";
      };
      Service = {
        ExecStart = escapeExecArgs args;
        Restart = "on-failure";
        RestartSec = 5;
      };
      Install.WantedBy = [ cfg.systemdTarget ];
    };

    launchd.agents.rxwpe = lib.mkIf pkgs.stdenv.hostPlatform.isDarwin {
      enable = true;
      config = {
        ProgramArguments = args;
        RunAtLoad = true;
        KeepAlive = true;
        ProcessType = "Interactive";
        LimitLoadToSessionType = "Aqua";
        ThrottleInterval = 5;
      };
    };
  };
}
