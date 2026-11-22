#!/usr/bin/env bash
# Build the arm64 package, check that the Pi is not recording, install it there and show the state afterwards.
#   scripts/pi-deploy.sh astropi@astropi.local [--no-build] [--force]
# Needs the cross toolchain described in docs/project/development.md. Installing restarts the service: a running recording would end,
# so it refuses unless --force. Camera and mount reconnect by themselves; tracking, calibration and guiding start again by hand.
set -euo pipefail
cd "$(dirname "$0")/.."
HOST="${1:?usage: scripts/pi-deploy.sh user@host [--no-build] [--force]}"; shift || true
BUILD=1; FORCE=0
for a in "$@"; do case "$a" in --no-build) BUILD=0;; --force) FORCE=1;; *) echo "unknown option $a" >&2; exit 2;; esac; done
[ "$BUILD" = 1 ] && { (cd web && npm ci --no-audit --no-fund && npm run build); scripts/build-deb.sh --skip-web; }
DEB="$(ls -t dist/rustflare_*_arm64.deb | head -1)"; echo "package: $DEB"
STATE="$(ssh "$HOST" 'curl -sk -m 10 https://localhost:8443/api/status || curl -s -m 10 http://localhost:8080/api/status' | python3 -c 'import sys,json; print(json.load(sys.stdin)["capture"]["state"])' || echo unknown)"
echo "capture state on the Pi: $STATE"
if [ "$STATE" != "idle" ] && [ "$FORCE" != 1 ]; then echo "not idle: stop the recording first, or use --force" >&2; exit 1; fi
scp -q "$DEB" "$HOST:/tmp/rf.deb"
ssh "$HOST" 'sudo apt-get install -y --allow-downgrades -o Dpkg::Options::=--force-confold /tmp/rf.deb 2>&1 | tail -2; rm -f /tmp/rf.deb; sleep 8; systemctl is-active rustflare'
ssh "$HOST" 'curl -sk -m 10 https://localhost:8443/api/status || curl -s -m 10 http://localhost:8080/api/status' | python3 -c 'import sys,json; d=json.load(sys.stdin); print("camera:", d["camera"] and d["camera"]["name"], "| mount:", d["mount"] and d["mount"]["info"])'
