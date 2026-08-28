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

            meta = {
              description = "Git worktrees, plus the files that git ignores";
              homepage = "https://github.com/lorenzolfm/wt";
              license = pkgs.lib.licenses.mit;
              mainProgram = "wt";
              platforms = pkgs.lib.platforms.unix;
            };
          });
      in {
        _module.args.pkgs = import nixpkgs {
          inherit system;
          overlays = [(import rust-overlay)];
        };

        checks = {
          inherit wt;

          # Separate derivations, so a lint failure blocks CI without it
          # blocking a user who only wants to build the crate.
          wt-clippy = craneLib.cargoClippy (commonArgs
            // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets -- --deny warnings";
            });

          wt-fmt = craneLib.cargoFmt {inherit src;};
        };

        packages.default = wt;

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
