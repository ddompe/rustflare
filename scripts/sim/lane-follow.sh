#!/bin/bash
# Scenario against the simulators (no hardware): needs `cargo build --release -p rustflare-server` and `cd web && npm run build` first.
R="$(cd "$(dirname "$0")/../.." && pwd)"; T="${TMPDIR:-/tmp}/rustflare-sim/lane-follow"; rm -rf "$T"; mkdir -p "$T"
"$R/target/release/rustflare-server" --bind 127.0.0.1:18118 --web-dir "$R/web/dist" --data-dir "$T/data" --config-file "$T/c.json" >"$T/log" 2>&1 & PID=$!
trap 'kill $PID 2>/dev/null' EXIT; sleep 1.5
A=http://127.0.0.1:18118/api; J='content-type: application/json'
post(){ curl -s -X "$1" -H "$J" -d "${3:-{\}}" "$A$2"; echo; }
post POST /camera/open '{"driver":"simulator","id":"sim0"}' >/dev/null
post PUT /camera/control '{"id":"sim_zoom","value":0.9}' >/dev/null
post POST /mount/connect '{"driver":"simulator"}' >/dev/null
post POST /mount/tracking '{"enabled":true}' >/dev/null; sleep 2
post POST /tracking/start '{"mode":"limb"}' >/dev/null; sleep 2
post POST /capture/lanes '{"name":"p","cycles":600,"cycle_interval_s":20,"format":"ser","lanes":[{"name":"disc","exposure_us":2000,"frames":25},{"name":"prom","exposure_us":30000,"frames":10,"mask_disc":true}]}' >/dev/null
sleep 6
echo "-- slewing the primary axis for 8 s (the Sun moves ~16 px/s)"; post POST /mount/slew '{"axis":"primary","speed":1}' >/dev/null
for i in $(seq 1 14); do
  sleep 1
  curl -s $A/status | python3 -c "import sys,json; s=json.load(sys.stdin); print('  t+$i s model x %.0f y %.0f  (lane exposure %s)' % (s['sun']['pos']['x'], s['sun']['pos']['y'], [c['value'] for c in s['camera']['controls'] if c['id']=='exposure_us'][0]))"
  [ $i -eq 8 ] && post POST /mount/slew '{"axis":"primary","speed":0}' >/dev/null
done
post POST /capture/stop >/dev/null
