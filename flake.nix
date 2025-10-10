{
  description = "rnostr-geo development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rustfmt" "clippy" ];
        };
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            # Rust toolchain
            rustToolchain

            # Build dependencies
            pkg-config
            cmake

            # System libraries for LMDB
            lmdb

            # Dependencies for lindera-unidic (Japanese text processing)
            openssl
            zlib
            libxml2
            libxslt
            curl
            wget
            unzip

            # Additional tools
            git
          ];

          # Environment variables
          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
          PKG_CONFIG_PATH = "${pkgs.openssl.dev}/lib/pkgconfig:${pkgs.lmdb}/lib/pkgconfig";

          # For lindera-unidic build script
          LINDERA_WORKDIR = "/tmp/lindera";

          shellHook = ''
            echo "rnostr-geo development environment"
            echo "Rust version: $(rustc --version)"

            # Create work directory for lindera
            mkdir -p $LINDERA_WORKDIR

            # Export additional environment variables for the build
            export OPENSSL_DIR="${pkgs.openssl.dev}"
            export OPENSSL_LIB_DIR="${pkgs.openssl.out}/lib"
            export OPENSSL_INCLUDE_DIR="${pkgs.openssl.dev}/include"
          '';
        };
      });
}