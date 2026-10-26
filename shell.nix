{ pkgs ? import <nixpkgs> {} }:
pkgs.mkShell {
  packages = with pkgs; [ cargo rustc rustfmt clippy pkg-config ];
  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (with pkgs; [
    libGL wayland libxkbcommon libx11 libxcursor libxi libxrandr
  ]);
}
