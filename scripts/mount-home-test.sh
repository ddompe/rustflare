#!/usr/bin/env bash
# REAL HARDWARE TEST: nudge the mount's primary axis a little, then send it home and watch it arrive.
# Moves the mount (about half a degree out, then back to its zero position). Watch the mount and cables.
#   scripts/mount-home-test.sh --yes [http://astropi.local:8080]
set -u
[ "${1:-}" = "--yes" ] || { echo "This moves the real mount. Re-run with --yes when the scope and cables are clear." >&2; exit 2; }
URL="${2:-http://astropi.local:8080}"
A="$URL/api"
post() { curl -s -X POST -H 'content-type: application/json' -d "${2:-{\}}" "$A$1"; echo; }
show() {
  curl -s "$A/status" | python3 -c '
import sys, json
m = json.load(sys.stdin)["mount"]
print("  tracking=%s slewing=%s at_home=%s park=%s" % (m["tracking"], m["slewing"], m["at_home"], m["park"]))'
}

echo "== before"; show
echo "== nudge: primary axis 60x sidereal for 2 s"
post /mount/slew '{"axis":"primary","speed":60}'
sleep 2
post /mount/slew '{"axis":"primary","speed":0}'
sleep 3
echo "== after the nudge (should NOT be at home)"; show
echo "== Send home"
post /mount/home '{"confirm":true}'
for i in $(seq 1 40); do
  sleep 2
  S=$(curl -s "$A/status" | python3 -c 'import sys,json; m=json.load(sys.stdin)["mount"]; print("%s %s" % (m["slewing"], m["at_home"]))')
  echo "  t+$((i*2))s slewing/at_home = $S"
  case "$S" in "False True") break;; esac
done
echo "== after"; show
