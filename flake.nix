{
  description = "Alexdelia profile";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
    git-hooks = {
      url = "github:cachix/git-hooks.nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      rust-overlay,
      crane,
      git-hooks,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
        toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        craneLib = (crane.mkLib pkgs).overrideToolchain (_: toolchain);

        isAsset = path: _type: builtins.match ".*/crate/profile-gen/asset(/.*)?" path != null;

        src = pkgs.lib.cleanSourceWith {
          src = ./.;
          filter = path: type: (craneLib.filterCargoSources path type) || (isAsset path type);
          name = "source";
        };

        baseBuild = {
          inherit src;
          pname = "profile-gen";
          version = (fromTOML (builtins.readFile ./Cargo.toml)).workspace.package.version;
          strictDeps = true;
        };

        cargoArtifacts = craneLib.buildDepsOnly baseBuild;
        cachedBuild = baseBuild // {
          inherit cargoArtifacts;
        };

        hook = import ./nix/hook {
          inherit
            git-hooks
            system
            pkgs
            toolchain
            ;
          src = ./.;
        };
      in
      {
        packages.default = craneLib.buildPackage (
          cachedBuild
          // {
            doCheck = false;
            cargoExtraArgs = "--locked --package profile-gen";
          }
        );

        checks = {
          package = self.packages.${system}.default;
          pre-commit = hook;
          machete = craneLib.mkCargoDerivation (
            baseBuild
            // {
              inherit cargoArtifacts;
              pnameSuffix = "-machete";
              doInstallCargoArtifacts = false;
              nativeBuildInputs = [ pkgs.cargo-machete ];
              buildPhaseCargoCommand = "cargo-machete";
            }
          );
          fmt = craneLib.cargoFmt baseBuild;
          clippy = craneLib.mkCargoDerivation (
            cachedBuild
            // {
              pnameSuffix = "-clippy";
              doInstallCargoArtifacts = false;
              buildPhaseCargoCommand = "cargo clippy --locked --all-targets -- --deny warnings";
            }
          );
          coverage = craneLib.cargoLlvmCov (
            cachedBuild
            // {
              cargoLlvmCovCommand = "nextest";
              cargoLlvmCovExtraArgs = "--lcov --output-path $out";
              postBuild = "cargoWithProfile llvm-cov report --fail-under-lines 100 --fail-under-functions 100 --fail-under-regions 100 --show-missing-lines";
              nativeBuildInputs = [
                pkgs.cargo-nextest
                pkgs.git
              ];
            }
          );
          deny = craneLib.cargoDeny (
            baseBuild
            // {
              cargoDenyChecks = "bans licenses sources";
            }
          );
        };

        devShells.default = pkgs.mkShell {
          inherit (hook) shellHook;
          packages =
            with pkgs;
            [
              nixfmt
              toolchain
              cargo-nextest
              cargo-llvm-cov
              cargo-deny
              cargo-machete
              git
            ]
            ++ hook.enabledPackages;
        };
      }
    );
}
