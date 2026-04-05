# Standalone derivation for use in NixOS/home-manager configs.
#
# Usage with fetchFromGitHub:
#
#   let
#     fuckimlate = pkgs.callPackage (import (pkgs.fetchFromGitHub {
#       owner = "soenkeliebau";
#       repo = "fuckimlate";
#       rev = "main";
#       hash = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
#     } + "/nix/package.nix")) {};
#   in
#   {
#     environment.systemPackages = [ fuckimlate ];
#   }
#
# Or if you already have the flake as an input, prefer using the flake package
# output instead of this file.
{ lib
, rustPlatform
, pkg-config
, dbus
, makeWrapper
, libnotify
, xdg-utils
}:

rustPlatform.buildRustPackage {
  pname = "fuckimlate";
  version = "0.1.0";

  src = lib.cleanSource ./..;

  # Replace with the real hash after first build attempt.
  # Run `nix-build` once and Nix will report the correct hash.
  cargoHash = lib.fakeHash;

  nativeBuildInputs = [
    pkg-config
    makeWrapper
  ];

  buildInputs = [
    dbus
  ];

  postInstall = ''
    wrapProgram "$out/bin/fuckimlate" \
      --prefix PATH : ${lib.makeBinPath [
        libnotify
        xdg-utils
      ]}
  '';

  meta = with lib; {
    description = "Conference call dialer for Google Calendar";
    homepage = "https://github.com/soenkeliebau/fuckimlate";
    license = licenses.asl20;
    mainProgram = "fuckimlate";
    platforms = platforms.linux;
  };
}
