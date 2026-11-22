#!/bin/bash
# Scenario against the simulators (no hardware): needs `cargo build --release -p rustflare-server` and `cd web && npm run build` first.
R="$(cd "$(dirname "$0")/../.." && pwd)"; T="${TMPDIR:-/tmp}/rustflare-sim/focus"; rm -rf "$T"; mkdir -p "$T/data"
"$R/target/release/rustflare-server" --bind 127.0.0.1:18115 --web-dir "$R/web/dist" --data-dir "$T/data" --config-file "$T/c.json" >"$T/log" 2>&1 & PID=$!
trap 'kill $PID 2>/dev/null' EXIT; sleep 1.5
A=http://127.0.0.1:18115/api; J='content-type: application/json'
curl -s -X POST -H "$J" -d '{"driver":"simulator","id":"sim0"}' $A/camera/open >/dev/null; sleep 2
curl -s -X PUT -H "$J" -d '{"enabled":true}' $A/focus; echo
for d in 0 3 8 15 8 3 0; do
  curl -s -X PUT -H "$J" -d "{\"id\":\"sim_defocus\",\"value\":$d}" $A/camera/control >/dev/null; sleep 3.2
  curl -s $A/status | python3 -c "import sys,json; f=json.load(sys.stdin)['focus']; print('defocus $d -> live %.2f lucky %.2f best %.2f n=%d' % (f['live'] or 0, f['lucky'] or 0, f['best'] or 0, len(f['history'])))"
done
curl -s -X PUT -H "$J" -d '{"enabled":false}' $A/focus; curl -s $A/status | python3 -c "import sys,json; print('off:', json.load(sys.stdin)['focus']['enabled'])"
