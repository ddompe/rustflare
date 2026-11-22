#!/bin/bash
# The recorder and the stacker together on the simulators: .ready markers, the capture-state file, the stacker pausing while a
# recording writes frames, then stacking everything once it is idle (and doing nothing the second time).
R="$(cd "$(dirname "$0")/../.." && pwd)"; T="${TMPDIR:-/tmp}/rustflare-sim/stacker"; rm -rf "$T"; mkdir -p "$T/cfg" "$T/data"
S="$R/target/release/rustflare-stack"
"$R/target/release/rustflare-server" --bind 127.0.0.1:18123 --web-dir "$R/web/dist" --data-dir "$T/data" --config-file "$T/cfg/c.json" >"$T/log" 2>&1 & PID=$!
trap 'kill $PID 2>/dev/null' EXIT; sleep 1.5
A=http://127.0.0.1:18123/api; J='content-type: application/json'; FAIL=0
post(){ curl -s -X POST -H "$J" -d "${2:-{\}}" "$A$1" >/dev/null; }
ok(){ if [ "$2" = "1" ]; then echo "ok   $1"; else echo "FAIL $1"; FAIL=1; fi; }
js(){ python3 -c "import json,sys; d=json.load(open('$1')); print($2)" 2>/dev/null; }
post /camera/open '{"driver":"simulator","id":"sim0"}'; sleep 2
post /capture/bursts '{"name":"st","frames":24,"interval_s":3,"bursts":3,"format":"ser"}'
for i in $(seq 1 40); do sleep 1; [ "$(curl -s $A/status | python3 -c "import sys,json; print(json.load(sys.stdin)['capture']['state'])")" = idle ] && [ "$(ls $T/data/*/*.ser 2>/dev/null | wc -l)" = 3 ] && break; done
sleep 2
ok "three recordings, each with a .ready marker" "$([ "$(ls $T/data/*/*.ser.ready 2>/dev/null | wc -l | tr -d ' ')" = 3 ] && echo 1)"
ok "the markers say how many frames the file holds" "$(grep -q '"frames":24' $(ls $T/data/*/*.ser.ready | head -1) && echo 1)"
ok "capture.state exists and says idle and where the data is" "$([ "$(js $T/cfg/capture.state "d['state']+'|'+str(d['busy'])")" = "idle|False" ] && echo 1)"
# a long recording is writing frames: the stacker must not start
post /capture/sequence '{"name":"long","frames":600,"format":"ser"}'; sleep 3
"$S" watch --once --state-dir "$T/cfg" >"$T/w1" 2>&1
ok "paused while a recording writes frames" "$([ "$(js $T/cfg/stack-status.json "d['state']")" = paused ] && echo 1)"
ok "nothing was stacked while paused" "$([ ! -e $T/data/stacks/manifest.json ] && echo 1)"
curl -s -X POST $A/capture/stop >/dev/null; for i in $(seq 1 30); do sleep 1; [ "$(js $T/cfg/capture.state "d['state']")" = idle ] && break; done; sleep 12
"$S" watch --once --state-dir "$T/cfg" >"$T/w2" 2>&1
N=$(ls $T/data/stacks/*/stack_*.fits 2>/dev/null | wc -l | tr -d ' ')
ok "stacked all the finished recordings once idle ($N)" "$([ "$N" -ge 3 ] && echo 1)"
ok "the manifest lists them" "$([ "$(js $T/data/stacks/manifest.json 'len(d)')" -ge 3 ] && echo 1)"
ok "status says idle with nothing left" "$([ "$(js $T/cfg/stack-status.json "d['state']+'|'+str(d['pending'])")" = "idle|0" ] && echo 1)"
M1=$(stat -f %m $T/data/stacks/manifest.json 2>/dev/null || stat -c %Y $T/data/stacks/manifest.json); sleep 1.2
"$S" watch --once --state-dir "$T/cfg" >"$T/w3" 2>&1
ok "a second run does no work" "$([ "$M1" = "$(stat -f %m $T/data/stacks/manifest.json 2>/dev/null || stat -c %Y $T/data/stacks/manifest.json)" ] && echo 1)"
echo "-- $(basename "$0"): $([ $FAIL = 0 ] && echo passed || echo FAILED)"; exit $FAIL
