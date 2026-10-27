{ pkgs ? import <nixpkgs> {} }:
pkgs.mkShell {
  packages = with pkgs; [ cargo rustc rustfmt clippy pkg-config fontconfig ]
    ++ lib.optionals stdenv.isLinux [ glib desktop-file-utils shellcheck ];
  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (with pkgs; [
    libGL wayland libxkbcommon libx11 libxcursor libxi libxrandr
  ]);
}
