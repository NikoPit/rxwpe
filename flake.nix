{
  description = "Rust player for Wallpaper Engine projects";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      eachSystem = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = eachSystem (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        rec {
          rxwpe = pkgs.callPackage ./nix/package.nix { };
          default = rxwpe;
        }
      );

      apps = eachSystem (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/rxwpe";
        };
      });

      devShells = eachSystem (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          default = pkgs.callPackage ./nix/dev-shell.nix { };
        in
        {
          inherit default;
        }
        // nixpkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
          # CI tests only: force software OpenGL so rendering tests need no GPU.
          # Use the default shell for development with hardware acceleration.
          ci = pkgs.mkShell {
            inputsFrom = [ default ];
            inherit (default) LD_LIBRARY_PATH;
            # Use Mesa from the same nixpkgs as the shell's GL loader.
            LIBGL_ALWAYS_SOFTWARE = "1";
            LIBGL_DRIVERS_PATH = "${pkgs.mesa}/lib/dri";
            __EGL_VENDOR_LIBRARY_FILENAMES = "${pkgs.mesa}/share/glvnd/egl_vendor.d/50_mesa.json";
            WGPU_BACKEND = "gl";
          };
        }
      );

      formatter = eachSystem (system: nixpkgs.legacyPackages.${system}.nixfmt-tree);

      checks = eachSystem (system: {
        configuration = import ./nix/tests.nix {
          pkgs = nixpkgs.legacyPackages.${system};
        };
      });

      homeManagerModules = {
        default = ./nix/home-manager.nix;
      };

    };
}
