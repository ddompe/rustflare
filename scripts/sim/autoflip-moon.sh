#!/bin/bash
# Scenario against the simulators (no hardware): the Moon profile's automatic meridian flip follows the MOON's meridian and
# restores the LUNAR tracking rate. The simulated camera shows a disc, which stands in for the Moon.
# Needs `cargo build --release -p rustflare-server` and `cd web && npm run build` first.
R="$(cd "$(dirname "$0")/../.." && pwd)"; T="${TMPDIR:-/tmp}/rustflare-sim/autoflip-moon"; rm -rf "$T"; mkdir -p "$T"
"$R/target/release/rustflare-server" --bind 127.0.0.1:18114 --web-dir "$R/web/dist" --data-dir "$T/data" --config-file "$T/c.json" >"$T/log" 2>&1 & PID=$!
trap 'kill $PID 2>/dev/null' EXIT; sleep 1.5
A=http://127.0.0.1:18114/api; J='content-type: application/json'
post(){ curl -s -X "$1" -H "$J" -d "${3:-{\}}" "$A$2"; echo; }
st(){ curl -s $A/status | python3 -c "import sys,json; s=json.load(sys.stdin); $1"; }
post PUT /profile '{"id":"moon"}' >/dev/null
LON=0
for i in 1 2 3; do   # the Moon crosses the meridian in 0.6 min: move the site east or west by the minutes left (4 min per degree, a little more for the Moon)
  post PUT /site "{\"latitude\":40,\"longitude\":$LON}" >/dev/null
  M=$(st 'print(s["flip_target"]["minutes_to_meridian"])')
  LON=$(python3 -c "print($LON + ($M-0.6)/4*0.966)")
done
echo "target: $(st 'print(s["flip_target"]["name"], s["flip_target"]["rate"], round(s["flip_target"]["minutes_to_meridian"],2), "min; the Sun is", round(s["sky_sun"]["minutes_to_meridian"],1), "min")')"
post POST /camera/open '{"driver":"simulator","id":"sim0"}' >/dev/null
post PUT /camera/control '{"id":"sim_zoom","value":0.9}' >/dev/null
post POST /mount/connect '{"driver":"simulator"}' >/dev/null
post POST /mount/tracking '{"enabled":true}' >/dev/null; sleep 2
post POST /tracking/start '{"mode":"limb"}' >/dev/null; sleep 1
post PUT /mount/autoflip '{"enabled":true,"record_min":5,"flip_min":0.1}'
for i in $(seq 1 40); do sleep 3; S=$(st 'f=s["flip"]; print(s["auto_flip"]["stage"], f["phase"] if f else "-", round(s["flip_target"]["minutes_to_meridian"],2), s["mount"]["rate"], s["error"])'); echo "  $S"; case "$S" in done\ -*) break;; esac; done
st 'l=s["last_flip"]; print("result:", l.get("result")); print("recorded for:", l["pre"].get("target")); print("mount rate after the flip:", s["mount"]["rate"])'
