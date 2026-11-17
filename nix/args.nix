{ lib, package }:

wallpaper:
[
  (lib.getExe package)
  (toString wallpaper.wallpaper)
]
++ lib.optionals (wallpaper.fps != null) [
  "--fps"
  (toString wallpaper.fps)
]
++ lib.optionals (wallpaper.scale != null) [
  "--scale"
  wallpaper.scale
]
++ lib.optionals (wallpaper.display != null) [
  "--display"
  wallpaper.display
]
++ lib.optionals (wallpaper.assets != null) [
  "--assets"
  (toString wallpaper.assets)
]
++ lib.concatLists (
  map (library: [
    "--steam-library"
    (toString library)
  ]) wallpaper.steamLibraries
)
++ lib.optionals (wallpaper.size != null) [
  "--size"
  wallpaper.size
]
++ lib.optionals wallpaper.allowPartial [ "--allow-partial" ]
++ lib.concatLists (
  lib.mapAttrsToList (name: value: [
    "--property"
    "${name}=${value}"
  ]) wallpaper.properties
)
++ wallpaper.extraArgs
