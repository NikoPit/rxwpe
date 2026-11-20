{ pkgs }:

let
  inherit (pkgs) lib;
  package = pkgs.runCommand "rxwpe-test-package" { meta.mainProgram = "rxwpe"; } ''
    mkdir -p $out/bin
    touch $out/bin/rxwpe
  '';
  executable = lib.getExe package;

  # Declare only the Home Manager options consumed or assigned by this module.
  # No Home Manager download or platform-specific build is needed for evaluation.
  evaluate = system: service:
    (lib.evalModules {
      specialArgs.pkgs = import pkgs.path { inherit system; };
      modules = [
        ./home-manager.nix
        {
          options = {
            home.packages = lib.mkOption {
              type = lib.types.listOf lib.types.package;
              default = [ ];
            };
            systemd.user.services = lib.mkOption {
              type = lib.types.attrsOf lib.types.anything;
              default = { };
            };
            launchd.agents = lib.mkOption {
              type = lib.types.attrsOf lib.types.anything;
              default = { };
            };
            wayland.systemd.target = lib.mkOption {
              type = lib.types.str;
              default = "graphical-session.target";
            };
          };
          config.services.rxwpe = { inherit package; } // service;
        }
      ];
    }).config;

  minimal = {
    enable = true;
    settings.wallpaper = "123456789";
  };
  linux = evaluate "x86_64-linux" minimal;
  darwin = evaluate "aarch64-darwin" minimal;
  disabledLinux = evaluate "x86_64-linux" { enable = false; };
  disabledDarwin = evaluate "aarch64-darwin" { enable = false; };

  configured = {
    enable = true;
    systemdTarget = "sway-session.target";
    settings = {
      wallpaper = "/wallpapers/a space/\"quote\"/$HOME/100%";
      fps = 60;
      scale = "fit";
      display = "DP-1";
      assets = "/shared assets";
      steamLibraries = [ "/first library" "/second library" ];
      size = "1920x1080";
      allowPartial = true;
      properties = {
        opacity = "0.5";
        text = "a \"quote\" \\ $HOME %n";
      };
      extraArgs = [ "--window" ];
    };
  };
  configuredLinux = evaluate "x86_64-linux" configured;
  configuredDarwin = evaluate "aarch64-darwin" configured;
  expectedArgs = [
    executable
    "/wallpapers/a space/\"quote\"/$HOME/100%"
    "--fps" "60"
    "--scale" "fit"
    "--display" "DP-1"
    "--assets" "/shared assets"
    "--steam-library" "/first library"
    "--steam-library" "/second library"
    "--size" "1920x1080"
    "--allow-partial"
    "--property" "opacity=0.5"
    "--property" "text=a \"quote\" \\ $HOME %n"
    "--window"
  ];
  arguments = settings: (import ./args.nix { inherit lib package; }) settings;
  tests = {
    testDefaultArguments = {
      expr = arguments linux.services.rxwpe.settings;
      expected = [ executable "123456789" ];
    };
    testAllArguments = {
      expr = arguments configuredLinux.services.rxwpe.settings;
      expected = expectedArgs;
    };
    testLinuxService = {
      expr = linux.systemd.user.services.rxwpe;
      expected = {
        Unit = {
          Description = "rxwpe wallpaper";
          After = [ "graphical-session.target" ];
          PartOf = [ "graphical-session.target" ];
          ConditionEnvironment = "WAYLAND_DISPLAY";
        };
        Service = {
          ExecStart = "\"${executable}\" \"123456789\"";
          Restart = "on-failure";
          RestartSec = 5;
        };
        Install.WantedBy = [ "graphical-session.target" ];
      };
    };
    testSystemdEscaping = {
      expr = configuredLinux.systemd.user.services.rxwpe.Service.ExecStart;
      expected = ''"${executable}" "/wallpapers/a space/\"quote\"/$$HOME/100%%" "--fps" "60" "--scale" "fit" "--display" "DP-1" "--assets" "/shared assets" "--steam-library" "/first library" "--steam-library" "/second library" "--size" "1920x1080" "--allow-partial" "--property" "opacity=0.5" "--property" "text=a \"quote\" \\ $$HOME %%n" "--window"'';
    };
    testCustomSessionTarget = {
      expr = with configuredLinux.systemd.user.services.rxwpe; [ Unit.After Unit.PartOf Install.WantedBy ];
      expected = lib.replicate 3 [ "sway-session.target" ];
    };
    testDarwinService = {
      expr = darwin.launchd.agents.rxwpe;
      expected = {
        enable = true;
        config = {
          ProgramArguments = [ executable "123456789" ];
          RunAtLoad = true;
          KeepAlive = true;
          ProcessType = "Interactive";
          LimitLoadToSessionType = "Aqua";
          ThrottleInterval = 5;
        };
      };
    };
    testLaunchdPreservesArguments = {
      expr = configuredDarwin.launchd.agents.rxwpe.config.ProgramArguments;
      expected = expectedArgs;
    };
    testOnlyNativeServiceIsGenerated = {
      expr = [ linux.launchd.agents darwin.systemd.user.services ];
      expected = [ { } { } ];
    };
    testPackageInstalled = {
      expr = [ linux.home.packages darwin.home.packages ];
      expected = [ [ package ] [ package ] ];
    };
    testDisabledModuleNeedsNoWallpaper = {
      expr = map (config: [ config.home.packages config.systemd.user.services config.launchd.agents ]) [ disabledLinux disabledDarwin ];
      expected = lib.replicate 2 [ [ ] { } { } ];
    };
    testInvalidFrameRateRejected = {
      expr = map (fps: (builtins.tryEval (builtins.deepSeq (evaluate "x86_64-linux" {
        enable = true;
        settings = { wallpaper = "123"; inherit fps; };
      }).services.rxwpe.settings true)).success) [ 0 241 ];
      expected = [ false false ];
    };
  };
  failures = lib.runTests tests;
in
if failures != [ ] then
  throw "rxwpe configuration tests failed: ${builtins.toJSON failures}"
else
  pkgs.runCommand "rxwpe-configuration-tests" { } ''
    touch $out
  ''
