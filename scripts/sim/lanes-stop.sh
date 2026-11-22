#!/bin/bash
# Scenario against the simulators (no hardware): needs `cargo build --release -p rustflare-server` and `cd web && npm run build` first.
R="$(cd "$(dirname "$0")/../.." && pwd)"; T="${TMPDIR:-/tmp}/rustflare-sim/lanes-stop"; rm -rf "$T"; mkdir -p "$T/data"
"$R/target/release/rustflare-server" --bind 127.0.0.1:18115 --web-dir "$R/web/dist" --data-dir "$T/data" --config-file "$T/c.json" >"$T/log" 2>&1 & PID=$!
trap 'kill $PID 2>/dev/null' EXIT; sleep 1.5
A=http://127.0.0.1:18115/api; J='content-type: application/json'
post(){ curl -s -X "$1" -H "$J" -d "${3:-{\}}" "$A$2"; echo; }
exp(){ curl -s $A/status | python3 -c 'import sys,json; s=json.load(sys.stdin); c=s["capture"]; e=[x["value"] for x in s["camera"]["controls"] if x["id"]=="exposure_us"][0]; print(c["state"], c["phase"], "exp", e)'; }
post POST /camera/open '{"driver":"simulator","id":"sim0"}' >/dev/null; sleep 1.5
post PUT /camera/control '{"id":"exposure_us","value":3000}' >/dev/null
for stopat in 1 3 6; do
  echo "== start, stop after ${stopat}s (cycle every 4 s, 600 cycles)"
  post POST /capture/lanes '{"name":"p","cycles":600,"cycle_interval_s":4,"format":"ser","lanes":[{"name":"disc","exposure_us":500,"frames":10},{"name":"prom","exposure_us":30000,"frames":10,"mask_disc":true}]}' >/dev/null
  sleep $stopat; post POST /capture/stop; 
  for i in 1 2 3 4 5 6; do sleep 2; echo "  after stop: $(exp)"; done
done
grep -c "tracking lost" "$T/log"
