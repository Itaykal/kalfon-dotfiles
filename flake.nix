{
  description = "dev-tools — standalone Rust CLIs (aws-switch, feature, wt-gc)";

  # Pinned to match the consumer flake (itaykal/nixos-config), which follows
  # nixpkgs nixos-26.05. Keep these in sync when bumping channels.
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    flake-utils.url = "github:numtide/flake-utils";
    # The workspace's MSRV (rust-version = 1.96) is newer than nixpkgs 26.05's
    # default rustc (1.95), so pin the toolchain from rust-overlay.
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { self, nixpkgs, flake-utils, rust-overlay, ... }:
    flake-utils.lib.eachSystem [ "aarch64-darwin" "x86_64-linux" ] (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
        # Newest stable toolchain (>= the 1.96 MSRV). Locked via flake.lock.
        rustToolchain = pkgs.rust-bin.stable.latest.default;
        rustPlatform = pkgs.makeRustPlatform {
          cargo = rustToolchain;
          rustc = rustToolchain;
        };
      in
      {
        # One derivation builds the whole Cargo workspace and installs every
        # `[[bin]]` crate (aws-switch, feature, wt-gc). The `common` library has
        # no bin, so buildRustPackage leaves it out of the install step.
        packages.dev-tools = rustPlatform.buildRustPackage {
          pname = "dev-tools";
          version = "0.2.0";

          # The Cargo workspace is the repo root.
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;

          # ureq builds against rustls (no OpenSSL/Security needed). libiconv
          # covers the darwin link step.
          buildInputs = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [
            pkgs.libiconv
          ];

          meta = {
            description = "Personal terminal CLIs: aws-switch, feature, wt-gc";
            platforms = pkgs.lib.platforms.unix;
          };
        };

        packages.default = self.packages.${system}.dev-tools;

        # `nix develop` for hacking on the workspace (replaces the `rust`
        # Brewfile entry for development).
        devShells.default = pkgs.mkShell {
          packages = [
            rustToolchain
            pkgs.rust-analyzer
          ];
        };
      }
    );
}
