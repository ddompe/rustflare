#!/bin/bash
# Scenario against the simulators (no hardware): needs `cargo build --release -p rustflare-server` and `cd web && npm run build` first.
R="$(cd "$(dirname "$0")/../.." && pwd)"; T="${TMPDIR:-/tmp}/rustflare-sim/lost-target"; rm -rf "$T"; mkdir -p "$T"
"$R/target/release/rustflare-server" --bind 127.0.0.1:18114 --web-dir "$R/web/dist" --data-dir "$T/data" --config-file "$T/c.json" >"$T/log" 2>&1 & PID=$!
trap 'kill $PID 2>/dev/null' EXIT; sleep 1.5
A=http://127.0.0.1:18114/api; J='content-type: application/json'
post(){ curl -s -X "$1" -H "$J" -d "${3:-{\}}" "$A$2"; echo; }
st(){ curl -s $A/status | python3 -c "import sys,json; s=json.load(sys.stdin); $1"; }
post POST /camera/open '{"driver":"simulator","id":"sim0"}' >/dev/null
post PUT /camera/control '{"id":"sim_zoom","value":0.9}' >/dev/null
post POST /mount/connect '{"driver":"simulator"}' >/dev/null
post POST /mount/tracking '{"enabled":true}' >/dev/null; sleep 2
post POST /tracking/start '{"mode":"limb"}' >/dev/null; sleep 1
post POST /guiding/calibrate >/dev/null
for i in $(seq 1 40); do sleep 2; S=$(st 'print(s["guiding"]["state"], bool(s["guiding"]["calibration"]))'); case "$S" in "off True") break;; esac; done
post POST /guiding/start '{"center":false}' >/dev/null; sleep 3
echo "guiding: $(st 'print(s["guiding"]["state"])')"
echo "-- lose the disc (zoom way in)"; post PUT /camera/control '{"id":"sim_zoom","value":8}' >/dev/null; sleep 6
st 'print("after loss:", s["guiding"]["state"], "held", s["guiding"]["held"], "lost", s["tracking"]["lost"], s["session_log"]["lost_saved"], "frames saved")'
echo "-- bring it back"; post PUT /camera/control '{"id":"sim_zoom","value":0.9}' >/dev/null; sleep 8
st 'print("after recovery:", s["guiding"]["state"], "held", s["guiding"]["held"], "lost", s["tracking"]["lost"])'
echo "== events"; cat "$T"/data/logs/*/events.jsonl | python3 -c 'import sys,json
for l in sys.stdin:
    d=json.loads(l); print(d["t"][11:23], d["event"], json.dumps(d["detail"])[:90])'
echo "== lost files"; ls "$T"/data/logs/*/lost/
cat "$T"/data/logs/*/lost/*.json | head -30
