#!/bin/bash
# Scenario against the simulators (no hardware): needs `cargo build --release -p rustflare-server` and `cd web && npm run build` first.
R="$(cd "$(dirname "$0")/../.." && pwd)"; T="${TMPDIR:-/tmp}/rustflare-sim/flip"; rm -rf "$T"; mkdir -p "$T"
"$R/target/release/rustflare-server" --bind 127.0.0.1:18112 --web-dir "$R/web/dist" --data-dir "$T/data" --config-file "$T/c.json" >"$T/log" 2>&1 & PID=$!
trap 'kill $PID 2>/dev/null' EXIT; sleep 1.5
A=http://127.0.0.1:18112/api; J='content-type: application/json'
post(){ curl -s -X "$1" -H "$J" -d "${3:-{\}}" "$A$2"; echo; }
st(){ curl -s $A/status | python3 -c "import sys,json; s=json.load(sys.stdin); $1"; }
post POST /camera/open '{"driver":"simulator","id":"sim0"}' >/dev/null
post PUT /camera/control '{"id":"sim_zoom","value":0.9}' >/dev/null
post POST /mount/connect '{"driver":"simulator"}' >/dev/null
post POST /mount/tracking '{"enabled":true}' >/dev/null; sleep 2
echo "refuse without Sun tracking? record needs Sun:"; post POST /mount/flip '{"record_only":true}'
post POST /tracking/start '{"mode":"limb"}' >/dev/null; sleep 1
post POST /guiding/calibrate >/dev/null
for i in $(seq 1 40); do sleep 2; S=$(st 'print(s["guiding"]["state"], bool(s["guiding"]["calibration"]))'); case "$S" in "off True") break;; esac; done; echo "calibrated: $S"
echo "flip without confirm:"; post POST /mount/flip '{}'
echo "restore with no record:"; post POST /mount/flip '{"restore":true,"confirm":true}'
echo "start flip:"; post POST /mount/flip '{"confirm":true}'
for i in $(seq 1 60); do sleep 2; S=$(st 'f=s["flip"]; print(f["phase"] if f else "idle", s["guiding"]["error_px"])'); echo "  $S"; case "$S" in idle*) break;; esac; done
st 'import json; print(json.dumps(s["last_flip"], indent=1))' | head -30
