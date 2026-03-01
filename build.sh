#!/usr/bin/env bash
# build.sh – Build the SvelteKit frontend, then compile the Rust binary.
#
# Usage:
#   ./build.sh            # debug build
#   ./build.sh --release  # release build (smaller, faster binary)
#
# The resulting binary is placed at:
#   target/debug/nixium     (debug)
#   target/release/nixium   (release)
#
# Run with:
#   ./target/release/nixium
#   NIXIUM_ADDR=0.0.0.0:8080 ./target/release/nixium

set -euo pipefail

RELEASE=""
CARGO_FLAGS=""

for arg in "$@"; do
  case $arg in
    --release)
      RELEASE="--release"
      CARGO_FLAGS="--release"
      ;;
  esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FRONTEND_DIR="$SCRIPT_DIR/frontend"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Step 1 – Install frontend dependencies"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
cd "$FRONTEND_DIR"

# npx npm-check-updates -u

# Use --frozen-lockfile only when a lockfile already exists (CI-safe).
if [[ -f pnpm-lock.yaml ]]; then
    pnpm install --no-frozen-lockfile
else
    pnpm install
fi

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Step 2 – Build SvelteKit SPA"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
pnpm run build

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Step 3 – Compile Rust binary${RELEASE:+ (release)}"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
cd "$SCRIPT_DIR"
cargo build $CARGO_FLAGS

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Step 4 – Install bundled extensions"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
EXT_SRC="$SCRIPT_DIR/extensions"
EXT_DST="${NIXIUM_EXTENSIONS_DIR:-$HOME/.config/nixium/extensions}"
mkdir -p "$EXT_DST"
for ext_dir in "$EXT_SRC"/*/; do
  ext_name="$(basename "$ext_dir")"
  dest="$EXT_DST/$ext_name"
  echo "  → $ext_name → $dest"
  cp -r "$ext_dir" "$dest"
done

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  ✓ Done!"
echo ""
if [[ -n "$RELEASE" ]]; then
  echo "  Binary: target/release/nixium"
else
  echo "  Binary: target/debug/nixium"
fi
echo ""
echo "  Run with:  ./target/${RELEASE:+release}${RELEASE:-debug}/nixium"
echo "  Override bind address with: NIXIUM_ADDR=0.0.0.0:8080"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
