{
  lib,
  stdenv,
  mkShell,
  callPackage,
  cargo,
  rustc,
  rustfmt,
  clippy,
  pkg-config,
  ffmpeg,
}:

mkShell (
  {
    packages = [
      cargo
      rustc
      rustfmt
      clippy
      pkg-config
      ffmpeg
    ];
  }
  // lib.optionalAttrs stdenv.hostPlatform.isLinux {
    LD_LIBRARY_PATH = lib.makeLibraryPath (callPackage ./runtime-libraries.nix { });
  }
)
