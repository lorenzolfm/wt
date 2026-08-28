{
  description = "Git worktrees, plus the ignored files they must share";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
      in
      {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "wt";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;

          # wt calls the git binary. Tests need it too.
          nativeCheckInputs = [ pkgs.git ];

          meta = {
            description = "Git worktrees, plus the ignored files they must share";
            homepage = "https://github.com/lorenzolfm/wt";
            license = pkgs.lib.licenses.mit;
            mainProgram = "wt";
            platforms = pkgs.lib.platforms.unix;
          };
        };

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            cargo
            rustc
            clippy
            rustfmt
            rust-analyzer
            git
          ];
        };

        formatter = pkgs.nixpkgs-fmt;
      });
}
