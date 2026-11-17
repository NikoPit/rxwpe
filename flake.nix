{
  description = "Rust player for Wallpaper Engine projects";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      eachSystem = nixpkgs.lib.genAttrs systems;
      runtimeLibraries = pkgs: with pkgs; [
        vulkan-loader
        libGL
        wayland
        libxkbcommon
        libx11
        libxcursor
        libxi
        libxrandr
      ];
    in {
      packages = eachSystem (system:
        let pkgs = import nixpkgs { inherit system; };
        in {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "rxwpe";
            version = "0.1.0";
            src = pkgs.lib.cleanSourceWith {
              src = ./.;
              filter = path: type:
                pkgs.lib.cleanSourceFilter path type
                && !(builtins.elem (baseNameOf path) [ "target" "result" ]);
            };
            cargoLock.lockFile = ./Cargo.lock;
            cargoBuildFlags = [ "--workspace" ];
            cargoTestFlags = [ "--workspace" ];
            nativeBuildInputs = [ pkgs.pkg-config pkgs.makeWrapper ];
            nativeCheckInputs = [ pkgs.ffmpeg ];
            postFixup = ''
              wrapProgram $out/bin/rxwpe \
                --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.ffmpeg ]} \
                --prefix LD_LIBRARY_PATH : ${pkgs.lib.makeLibraryPath (runtimeLibraries pkgs)}
            '';
            meta.mainProgram = "rxwpe";
          };
        });

      apps = eachSystem (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/rxwpe";
        };
      });

      devShells = eachSystem (system:
        let pkgs = import nixpkgs { inherit system; };
        in {
          default = pkgs.mkShell {
            packages = with pkgs; [ cargo rustc rustfmt clippy pkg-config ffmpeg ];
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (runtimeLibraries pkgs);
          };
        });
    };
}
