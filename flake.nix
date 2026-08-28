{
  description = "Git worktrees, plus the files that git ignores";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    rust-overlay.url = "github:oxalica/rust-overlay";
    crane.url = "github:ipetkov/crane";
  };

  outputs = inputs @ {
    flake-parts,
    nixpkgs,
    rust-overlay,
    crane,
    ...
  }:
    flake-parts.lib.mkFlake {inherit inputs;} {
      systems = [
        "x86_64-linux"
        "aarch64-darwin"
      ];

      perSystem = {
        system,
        pkgs,
        ...
      }: let
        craneLib = (crane.mkLib pkgs).overrideToolchain (p:
          p.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml);

        src = craneLib.cleanCargoSource ./.;

        commonArgs = {
          inherit src;
          strictDeps = true;

          # wt runs the git binary. The tests need it on PATH.
          nativeCheckInputs = [pkgs.git];
        };

        # Build only the dependencies, so CI can cache that work.
        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        wt = craneLib.buildPackage (commonArgs
          // {
            inherit cargoArtifacts;

            pname = "wt";

            # The test gate runs the tests. Do not run them two times.
            doCheck = false;

            meta = {
              description = "Git worktrees, plus the files that git ignores";
              homepage = "https://github.com/lorenzolfm/wt";
              license = pkgs.lib.licenses.mit;
              mainProgram = "wt";
              platforms = pkgs.lib.platforms.unix;
            };
          });

        # One derivation for each gate. CI builds them in parallel, and
        # `nix flake check` runs all of them.
        gates = {
          inherit wt;

          # Each gate is a separate derivation. A lint failure therefore stops
          # CI, but it does not stop a user who only wants to build the crate.
          wt-clippy = craneLib.cargoClippy (commonArgs
            // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets -- --deny warnings";
            });

          wt-test = craneLib.cargoNextest (commonArgs
            // {
              inherit cargoArtifacts;
              partitions = 1;
              partitionType = "count";
            });

          wt-fmt = craneLib.cargoFmt {inherit src;};
        };
      in {
        _module.args.pkgs = import nixpkgs {
          inherit system;
          overlays = [(import rust-overlay)];
        };

        checks = gates;

        # Give each gate a package name, so CI can build one gate with
        # `nix build .#<gate>`.
        packages =
          gates
          // {
            default = wt;
          };

        apps.default = {
          type = "app";
          program = "${pkgs.lib.getExe wt}";
        };

        devShells.default = craneLib.devShell {
          packages = with pkgs; [
            cargo-nextest
            git
          ];

          shellHook = ''
            echo "  Rust: $(rustc --version)"
          '';
        };

        formatter = pkgs.alejandra;
      };
    };
}
