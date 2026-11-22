#!/bin/bash
# Scenario against the simulators (no hardware): needs `cargo build --release -p rustflare-server` and `cd web && npm run build` first.
# guiding while a prominence lane recording runs: does the Sun stay at the target?
R="$(cd "$(dirname "$0")/../.." && pwd)"; T="${TMPDIR:-/tmp}/rustflare-sim/lanes-guide"; rm -rf "$T"; mkdir -p "$T"
"$R/target/release/rustflare-server" --bind 127.0.0.1:18117 --web-dir "$R/web/dist" --data-dir "$T/data" --config-file "$T/c.json" >"$T/log" 2>&1 & PID=$!
trap 'kill $PID 2>/dev/null' EXIT; sleep 1.5
A=http://127.0.0.1:18117/api; J='content-type: application/json'
post(){ curl -s -X "$1" -H "$J" -d "${3:-{\}}" "$A$2"; echo; }
st(){ curl -s $A/status | python3 -c "import sys,json,math; s=json.load(sys.stdin); $1"; }
post POST /camera/open '{"driver":"simulator","id":"sim0"}' >/dev/null
post PUT /camera/control '{"id":"sim_zoom","value":0.9}' >/dev/null
post POST /mount/connect '{"driver":"simulator"}' >/dev/null
post POST /mount/tracking '{"enabled":true}' >/dev/null; sleep 2
post POST /tracking/start '{"mode":"limb"}' >/dev/null; sleep 1
post POST /guiding/calibrate >/dev/null
for i in $(seq 1 40); do sleep 2; S=$(st 'print(s["guiding"]["state"], bool(s["guiding"]["calibration"]))'); case "$S" in "off True") break;; esac; done
post POST /guiding/start '{"center":false}' >/dev/null
echo "target: $(st 'print([round(v) for v in s["guiding"]["target"]])')"
post POST /capture/lanes '{"name":"p","cycles":600,"cycle_interval_s":12,"format":"ser","lanes":[{"name":"disc","exposure_us":2000,"frames":25},{"name":"prom","exposure_us":30000,"frames":10,"mask_disc":true}]}' >/dev/null
for i in $(seq 1 16); do sleep 15; st 'sun=s["sun"]["pos"]; t=s["guiding"]["target"]; print("  t+%ds offset %.1f px guiding %s err %s" % ('$i'*15, math.hypot(sun["x"]-t[0], sun["y"]-t[1]), s["guiding"]["state"], s["guiding"]["error_px"]))'; done
post POST /capture/stop >/dev/null
