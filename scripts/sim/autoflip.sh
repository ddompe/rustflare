#!/bin/bash
# Scenario against the simulators (no hardware): needs `cargo build --release -p rustflare-server` and `cd web && npm run build` first.
R="$(cd "$(dirname "$0")/../.." && pwd)"; T="${TMPDIR:-/tmp}/rustflare-sim/autoflip"; rm -rf "$T"; mkdir -p "$T"
"$R/target/release/rustflare-server" --bind 127.0.0.1:18113 --web-dir "$R/web/dist" --data-dir "$T/data" --config-file "$T/c.json" >"$T/log" 2>&1 & PID=$!
trap 'kill $PID 2>/dev/null' EXIT; sleep 1.5
A=http://127.0.0.1:18113/api; J='content-type: application/json'
post(){ curl -s -X "$1" -H "$J" -d "${3:-{\}}" "$A$2"; echo; }
st(){ curl -s $A/status | python3 -c "import sys,json; s=json.load(sys.stdin); $1"; }
post PUT /site '{"latitude":40,"longitude":0}' >/dev/null
M=$(st 'print(s["sky_sun"]["minutes_to_meridian"])')
LON=$(python3 -c "print(($M-0.6)/4)")   # the Sun crosses the meridian in 0.6 min
post PUT /site "{\"latitude\":40,\"longitude\":$LON}"
echo "minutes to meridian: $(st 'print(round(s["sky_sun"]["minutes_to_meridian"],2))')"
post POST /camera/open '{"driver":"simulator","id":"sim0"}' >/dev/null
post PUT /camera/control '{"id":"sim_zoom","value":0.9}' >/dev/null
post POST /mount/connect '{"driver":"simulator"}' >/dev/null
post POST /mount/tracking '{"enabled":true}' >/dev/null; sleep 2
post POST /tracking/start '{"mode":"limb"}' >/dev/null; sleep 1
post POST /guiding/calibrate >/dev/null
for i in $(seq 1 40); do sleep 2; S=$(st 'print(s["guiding"]["state"], bool(s["guiding"]["calibration"]))'); case "$S" in "off True") break;; esac; done; echo "calibrated: $S"
post POST /guiding/start '{"center":false}' >/dev/null; sleep 4
echo "min:"; st 'print(round(s["sky_sun"]["minutes_to_meridian"],2))'
post PUT /mount/autoflip '{"enabled":true,"record_min":5,"flip_min":0.1}'
for i in $(seq 1 60); do sleep 3; S=$(st 'f=s["flip"]; print(s["auto_flip"]["stage"], f["phase"] if f else "-", round(s["sky_sun"]["minutes_to_meridian"],2), s["guiding"]["error_px"], s["error"], s["guiding"]["state"])'); echo "  $S"; case "$S" in done\ -*) break;; esac; done
st 'import json; l=s["last_flip"]; print(l.get("result"), l.get("total_s"), l.get("image_turned_over"))'
grep -i "auto flip" "$T/log"
