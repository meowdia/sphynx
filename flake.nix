{
  description = "A very basic flake";

  inputs = {
    crane.url = "github:ipetkov/crane";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-25.11";
  };

  outputs =
    {
      self,
      crane,
      fenix,
      flake-utils,
      nixpkgs,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ fenix.overlays.default ];
        };

        craneLib = (crane.mkLib pkgs).overrideToolchain (pkgs.fenix.stable.toolchain);

        commonArgs = {
          src = craneLib.cleanCargoSource ./.;
          strictDeps = true;
        };

        cargoArtifacts = craneLib.buildDepsOnly (
          commonArgs
          // {
            pname = "sphynx-deps";
          }
        );

        sphynxClippy = craneLib.cargoClippy (
          commonArgs
          // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- --deny warnings";
          }
        );

        sphynxFmt = craneLib.cargoFmt (
          commonArgs
          // {
            inherit cargoArtifacts;
          }
        );

        sphynxTest = craneLib.cargoTest (
          commonArgs
          // {
            inherit cargoArtifacts;
          }
        );

        sphynx = craneLib.cargoBuild (
          commonArgs
          // {
            inherit cargoArtifacts;
          }
        );
      in
      {
        checks = {
          clippy = sphynxClippy;
          build = sphynx;
          test = sphynxTest;
          fmt = sphynxFmt;
        };

        apps = builtins.listToAttrs (
          builtins.map
            (
              name:
              let
                cmd = pkgs.writeShellScript "just-${name}" "${pkgs.just}/bin/just ${name}";
              in
              {
                inherit name;
                value = {
                  type = "app";
                  program = "${cmd}";
                  meta = {
                    description = ''runs `just ${name}`'';
                  };
                };
              }
            )
            [
              "build"
              "test"
              "lint"
              "check"
              "clean"
              "fmt"
            ]
        );

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            pkgs.fenix.stable.toolchain
            just
          ];
        };
      }
    );
}
