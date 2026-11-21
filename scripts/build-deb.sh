#!/usr/bin/env bash
# Cross-compile Rustflare for a Raspberry Pi (arm64) and build the .deb into ./dist/
#
# Needs: rustup target aarch64-unknown-linux-gnu, zig, cargo-zigbuild, cargo-deb, node+npm
#   rustup target add aarch64-unknown-linux-gnu
#   cargo install --locked cargo-zigbuild cargo-deb      (zig: https://ziglang.org/download/ or `pip install ziglang`)
# Usage: scripts/build-deb.sh [--skip-web] [--version X.Y.Z-N]
set -euo pipefail
cd "$(dirname "$0")/.."

TARGET=aarch64-unknown-linux-gnu
GLIBC=2.36        # Raspberry Pi OS bookworm and newer
SKIP_WEB=0
VERSION=""
while [ $# -gt 0 ]; do
  case "$1" in
    --skip-web) SKIP_WEB=1 ;;
    --version) VERSION="$2"; shift ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
  shift
done

if [ "$SKIP_WEB" = 0 ]; then
  (cd web && npm ci --no-audit --no-fund && npm run build)
fi
[ -f web/dist/index.html ] || { echo "web/dist missing: build the UI first" >&2; exit 1; }

# --features asi: ZWO ASI cameras, loads libASICamera2 (Debian: libasi, a Recommends) at run time, ADR 0014
# --features playerone: Player One camera support (bundles the vendor library, see ADR 0006)
cargo zigbuild --release -p rustflare-server --features playerone,asi --target "$TARGET.$GLIBC"

mkdir -p dist
DEB_ARGS=(--no-build --no-strip --target "$TARGET" -p rustflare-server --output dist/)
[ -n "$VERSION" ] && DEB_ARGS+=(--deb-version "$VERSION")
cargo deb "${DEB_ARGS[@]}"

(cd dist && for f in rustflare_*_arm64.deb; do sha256sum "$f" 2>/dev/null || shasum -a 256 "$f"; done > SHA256SUMS)
ls -la dist
