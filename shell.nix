{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
  nativeBuildInputs = with pkgs; [
    pkg-config
  ];

  buildInputs = with pkgs; [
    dbus
  ];

  packages = with pkgs; [
    libnotify
    xdg-utils
  ];
}
