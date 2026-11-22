#!/bin/bash
# Scenario against the simulators (no hardware): needs `cargo build --release -p rustflare-server` and `cd web && npm run build` first.
R="$(cd "$(dirname "$0")/../.." && pwd)"; T="${TMPDIR:-/tmp}/rustflare-sim/lanes"; rm -rf "$T"; mkdir -p "$T/data"
"$R/target/release/rustflare-server" --bind 127.0.0.1:18113 --web-dir "$R/web/dist" --data-dir "$T/data" --config-file "$T/c.json" >"$T/log" 2>&1 & PID=$!
trap 'kill $PID 2>/dev/null' EXIT; sleep 1.5
A=http://127.0.0.1:18113/api; J='content-type: application/json'
post(){ curl -s -X "$1" -H "$J" -d "${3:-{\}}" "$A$2"; echo; }
post POST /camera/open '{"driver":"simulator","id":"sim0"}' >/dev/null; sleep 1.5
echo "one lane:"; post POST /capture/lanes '{"lanes":[{"name":"a","exposure_us":100,"frames":3}]}'
echo "dup names:"; post POST /capture/lanes '{"lanes":[{"name":"a","exposure_us":100,"frames":3},{"name":"a","exposure_us":200,"frames":3}]}'
echo "start:"; post POST /capture/lanes '{"name":"eclipse","cycles":2,"format":"ser","lanes":[{"name":"short","exposure_us":200,"frames":5},{"name":"mid","exposure_us":2000,"gain":10,"frames":5},{"name":"long","exposure_us":20000,"seconds":0.5}]}'
for i in $(seq 1 30); do sleep 1; S=$(curl -s $A/status | python3 -c 'import sys,json; s=json.load(sys.stdin); c=s["capture"]; e=[x["value"] for x in s["camera"]["controls"] if x["id"]=="exposure_us"][0]; print(c["state"], c["phase"], c.get("burst"), c.get("bursts_total"), "exp", e)'); echo "  $S"; case "$S" in idle*) break;; esac; done
find "$T/data" -type f | sed "s|$T/data/||" | sort
