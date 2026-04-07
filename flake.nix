# SPDX-FileCopyrightText: 2026 Meowdia Community
# SPDX-License-Identifier: MIT OR Apache-2.0

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

        craneLib = (crane.mkLib pkgs).overrideToolchain (
          pkgs.fenix.stable.withComponents [
            "cargo"
            "rustc"
            "rustfmt"
            "clippy"
          ]
        );

        commonArgs = {
          src = pkgs.lib.fileset.toSource {
            root = ./.;
            fileset = pkgs.lib.fileset.unions [
              (craneLib.fileset.commonCargoSources ./.)
              ./iana
            ];
          };
          strictDeps = true;
        };

        cargoArtifacts = craneLib.buildDepsOnly (
          commonArgs
          // {
            cargoExtraArgs = "--workspace";
            pname = "sphynx";
          }
        );

        cargoArtifactsDev = cargoArtifacts.overrideAttrs (
          final: prev: {
            CARGO_PROFILE = "dev";
          }
        );

        sphynxClippy = craneLib.cargoClippy (
          commonArgs
          // {
            CARGO_PROFILE = "dev";
            cargoArtifacts = cargoArtifactsDev;
            cargoClippyExtraArgs = "--all-targets -- --deny warnings";
          }
        );

        sphynxFmt = craneLib.cargoFmt commonArgs;

        sphynxTest = craneLib.cargoTest (
          commonArgs
          // {
            CARGO_PROFILE = "dev";
            cargoArtifacts = cargoArtifactsDev;
          }
        );

        sphynx = craneLib.cargoBuild (
          commonArgs
          // {
            inherit cargoArtifacts;
          }
        );

        iana = craneLib.mkCargoDerivation (commonArgs // {
          pname = "xtask";
          cargoArtifacts = cargoArtifactsDev;
          CARGO_PROFILE = "dev";

          nativeBuildInputs = [ pkgs.git ];

          buildPhaseCargoCommand = "cargo run -p xtask -- iana check";
        });
      in
      {
        checks = {
          clippy = sphynxClippy;
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
                    description = "runs `just ${name}`";
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

        packages = {
          ci_clippy = sphynxClippy;
          ci_test = sphynxTest;
          ci_fmt = sphynxFmt;
          deps = cargoArtifacts;
          deps_dev = cargoArtifactsDev;
          iana_check = iana;
          lib = sphynx;
        };

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            pkgs.fenix.stable.toolchain
            just
            reuse
          ];
        };
      }
    );
}
