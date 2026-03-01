{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
  name = "nixium-dev";

  packages = with pkgs; [
    # ── Rust toolchain ──────────────────────────────────────────────
    cargo
    rustc
    rustfmt
    clippy
    # LSP support (used by rust-analyzer VS Code extension)
    rust-analyzer
    # Required by proc-macro crates at link time
    pkg-config

    # ── C libraries pulled in by Rust crates ───────────────────────
    # openssl-sys (transitive dep through some tower crates)
    openssl
    openssl.dev

    # ── Node.js (SvelteKit build) ───────────────────────────────────
    nodejs_22        # LTS; ships with npm
    nodePackages.pnpm

    # ── Helpful utilities ───────────────────────────────────────────
    git
    jq               # handy for inspecting API responses during dev
  ];

  # ── Environment ────────────────────────────────────────────────────
  # Tell pkg-config where to find OpenSSL headers so cargo build works
  # without any extra flags.
  PKG_CONFIG_PATH = "${pkgs.openssl.dev}/lib/pkgconfig";

  # Point rust-analyzer at the Rust standard library source.
  RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";

  # Emit backtraces on panic during development.
  RUST_BACKTRACE = "1";

  shellHook = ''
    echo ""
    echo "  ✦ nixium dev shell"
    echo "  ──────────────────────────────────"
    echo "  Rust  : $(rustc --version)"
    echo "  Cargo : $(cargo --version)"
    echo "  Node  : $(node --version)"
    echo "  npm   : $(npm --version)"
    echo "  pnpm  : $(pnpm --version)"
    echo "  ──────────────────────────────────"
    echo "  Build : ./build.sh --release"
    echo "  Dev   : cd frontend && npm run dev"
    echo ""
  '';
}
