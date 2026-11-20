{
  lib,
  stdenv,
  rustPlatform,
  callPackage,
  pkg-config,
  makeWrapper,
  ffmpeg,
  runtimeLibraries ? callPackage ./runtime-libraries.nix { },
}:

rustPlatform.buildRustPackage {
  pname = "rxwpe";
  version = "0.1.0";
  src = lib.cleanSourceWith {
    src = ../.;
    filter =
      path: type:
      lib.cleanSourceFilter path type
      && !(builtins.elem (baseNameOf path) [
        "target"
        "result"
      ]);
  };
  cargoLock.lockFile = ../Cargo.lock;
  cargoBuildFlags = [ "--workspace" ];
  cargoTestFlags = [ "--workspace" ];
  nativeBuildInputs = [
    pkg-config
    makeWrapper
  ];
  nativeCheckInputs = [ ffmpeg ];
  postFixup = ''
    wrapProgram $out/bin/rxwpe \
      --prefix PATH : ${lib.makeBinPath [ ffmpeg ]} ${lib.optionalString stdenv.hostPlatform.isLinux "--prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath runtimeLibraries}"}
  '';
  meta = {
    description = "Play Wallpaper Engine wallpapers on Linux and macOS";
    homepage = "https://github.com/NikoPit/rxwpe";
    license = lib.licenses.mit;
    mainProgram = "rxwpe";
    platforms = [
      "x86_64-linux"
      "aarch64-linux"
      "aarch64-darwin"
    ];
  };
}
