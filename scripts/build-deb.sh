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
# --features ble: Bluetooth mount link over BlueZ/D-Bus (pure Rust zbus, bluez is a Recommends), ADR 0015
# --features playerone: Player One camera support (bundles the vendor library, see ADR 0006)
cargo zigbuild --release -p rustflare-server --features playerone,asi,ble --target "$TARGET.$GLIBC"

mkdir -p dist
# A local build without --version must sort ABOVE the released version it is based on (dpkg: 0.2.6 < 0.2.6+dev...
# < 0.2.7), so it can be installed over it and rustflare-update still offers the next release.
if [ -z "$VERSION" ]; then
  WS=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
  VERSION="${WS}+dev$(date +%Y%m%d%H%M).$(git rev-parse --short HEAD 2>/dev/null || echo nogit)"
  echo "using dev version $VERSION"
fi
DEB_ARGS=(--no-build --no-strip --target "$TARGET" -p rustflare-server --output dist/)
[ -n "$VERSION" ] && DEB_ARGS+=(--deb-version "$VERSION")
cargo deb "${DEB_ARGS[@]}"

(cd dist && for f in rustflare_*_arm64.deb; do sha256sum "$f" 2>/dev/null || shasum -a 256 "$f"; done > SHA256SUMS)
ls -la dist
