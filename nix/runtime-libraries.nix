{
  lib,
  stdenv,
  vulkan-loader,
  libGL,
  wayland,
  libxkbcommon,
  libx11,
  libxcursor,
  libxi,
  libxrandr,
}:

lib.optionals stdenv.hostPlatform.isLinux [
  vulkan-loader
  libGL
  wayland
  libxkbcommon
  libx11
  libxcursor
  libxi
  libxrandr
]
