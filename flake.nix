{
  description = "fuckimlate - conference call dialer for Google Calendar";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, crane, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        craneLib = crane.mkLib pkgs;

        commonArgs = {
          src = craneLib.cleanCargoSource ./.;
          strictDeps = true;

          nativeBuildInputs = with pkgs; [
            pkg-config
          ];

          buildInputs = with pkgs; [
            dbus
          ] ++ lib.optionals stdenv.hostPlatform.isDarwin [
            darwin.apple_sdk.frameworks.Security
          ];
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        fuckimlate = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;

          postInstall = ''
            wrapProgram "$out/bin/fuckimlate" \
              --prefix PATH : ${pkgs.lib.makeBinPath (with pkgs; [
                libnotify
                xdg-utils
              ])}
          '';

          nativeBuildInputs = commonArgs.nativeBuildInputs ++ [ pkgs.makeWrapper ];

          meta = with pkgs.lib; {
            description = "Conference call dialer for Google Calendar";
            homepage = "https://github.com/soenkeliebau/fuckimlate";
            license = licenses.asl20;
            mainProgram = "fuckimlate";
          };
        });
      in
      {
        checks = {
          inherit fuckimlate;

          fuckimlate-clippy = craneLib.cargoClippy (commonArgs // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- -D warnings";
          });

          fuckimlate-fmt = craneLib.cargoFmt {
            src = craneLib.cleanCargoSource ./.;
          };

          fuckimlate-test = craneLib.cargoTest (commonArgs // {
            inherit cargoArtifacts;
          });
        };

        packages = {
          default = fuckimlate;
          fuckimlate = fuckimlate;
        };

        apps.default = flake-utils.lib.mkApp {
          drv = fuckimlate;
        };

        devShells.default = craneLib.devShell {
          checks = self.checks.${system};

          packages = with pkgs; [
            pkg-config
            dbus
            cargo-watch
            rust-analyzer
            libnotify
            xdg-utils
          ];
        };
      }
    );
}
