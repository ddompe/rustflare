#!/bin/bash
# After each burst (ADR 0019): the recorder and the real stacker together on the simulators, in each of the four modes, plus the
# cases that must never be quiet: asking for stack-only without a stacker, and the stacker dying in the middle of a recording.
R="$(cd "$(dirname "$0")/../.." && pwd)"; T="${TMPDIR:-/tmp}/rustflare-sim/after-burst"; rm -rf "$T"; mkdir -p "$T/cfg" "$T/data"
S="$R/target/release/rustflare-stack"
"$R/target/release/rustflare-server" --bind 127.0.0.1:18124 --web-dir "$R/web/dist" --data-dir "$T/data" --config-file "$T/cfg/c.json" >"$T/log" 2>&1 & PID=$!
SP=""; start_stacker(){ "$S" watch --state-dir "$T/cfg" >"$T/stacker.log" 2>&1 & SP=$!; }
trap 'kill $PID $SP 2>/dev/null' EXIT; sleep 1.5
A=http://127.0.0.1:18124/api; J='content-type: application/json'; FAIL=0
post(){ curl -s -X "${3:-POST}" -H "$J" -d "${2:-{\}}" "$A$1"; }
ok(){ if [ "$2" = "1" ]; then echo "ok   $1"; else echo "FAIL $1"; FAIL=1; fi; }
st(){ curl -s $A/status | python3 -c "import sys,json; d=json.load(sys.stdin); print($1)" 2>/dev/null; }
wait_idle(){ for i in $(seq 1 90); do sleep 1; [ "$(st "d['capture']['state']")" = idle ] && [ "$(st "d['after_burst']['stacking']")" = 0 ] && return; done; }
count(){ ls $1 2>/dev/null | wc -l | tr -d ' '; }
frames_of(){ python3 - "$1" <<'PY'
import struct,sys
h=open(sys.argv[1],'rb').read(178); print(struct.unpack('<4I',h[26:42])[3])
PY
}
post /camera/open '{"driver":"simulator","id":"sim0"}' >/dev/null; sleep 2

# no stacker yet: stack-only is refused, with the reason
R1=$(post /capture/after-burst '{"mode":"stack_only"}' PUT)
ok "stack-only is refused while the stacker is not running, and says why" "$(echo "$R1" | grep -q "stacker is not running" && echo 1)"
start_stacker; for i in $(seq 1 15); do [ "$(st "d['after_burst']['stacker_connected']")" = True ] && break; sleep 1; done
ok "the stacker connects" "$([ "$(st "d['after_burst']['stacker_connected']")" = True ] && echo 1)"

bursts(){ post /capture/bursts "{\"name\":\"$1\",\"frames\":24,\"interval_s\":3,\"bursts\":3,\"format\":\"ser\"}" >/dev/null; sleep 2; wait_idle; sleep 1; }

# 3: stack only
post /capture/after-burst '{"mode":"stack_only","stack_keep_percent":50}' PUT >/dev/null; bursts so
ok "stack only: three stacks in the manifest" "$([ "$(python3 -c "import json; print(sum('_so_' in e['source'] for e in json.load(open('$T/data/stacks/manifest.json'))))")" = 3 ] && echo 1)"
ok "stack only: no video written" "$([ "$(count "$T/data/*/*_so_*.ser")" = 0 ] && echo 1)"
ok "stack only: the stacks are FITS files" "$([ "$(count "$T/data/stacks/*/stack_*_so_*.fits")" = 3 ] && echo 1)"
ok "stack only: the burst's sidecar says where the stack is" "$(grep -q '"stack"' $(ls $T/data/*/*_so_*.json | head -1) && echo 1)"
ok "stack only: nothing is left in the buffer" "$([ "$(st "d['capture']['buffer_queued']")" = 0 ] && echo 1)"

# 2: video and stack
post /capture/after-burst '{"mode":"video_stack"}' PUT >/dev/null; N1=$(st "d['after_burst']['stacked']"); bursts vs
ok "video + stack: the stacks were made from memory (not by the disk watcher)" "$([ "$(st "d['after_burst']['stacked']")" = $((N1 + 3)) ] && echo 1)"
ok "video + stack: three videos" "$([ "$(count "$T/data/*/*_vs_*.ser")" = 3 ] && echo 1)"
ok "video + stack: three stacks" "$([ "$(count "$T/data/stacks/*/stack_*_vs_*.fits")" = 3 ] && echo 1)"
sleep 8
ok "video + stack: the stacker does not stack the videos a second time" "$([ "$(python3 -c "import json; print(sum('_vs_' in e['source'] for e in json.load(open('$T/data/stacks/manifest.json'))))")" = 3 ] && echo 1)"

# 4: best frames only (25 % of 24 frames = 6)
post /capture/after-burst '{"mode":"best_frames","raw_keep_percent":25}' PUT >/dev/null; bursts bf
F=$(ls $T/data/*/*_bf_*.ser 2>/dev/null | head -1)
ok "best frames: three small videos" "$([ "$(count "$T/data/*/*_bf_*.ser")" = 3 ] && echo 1)"
ok "best frames: each holds the best 25 % (6 of 24 frames)" "$([ -n "$F" ] && [ "$(frames_of "$F")" = 6 ] && echo 1)"
ok "best frames: three stacks" "$([ "$(count "$T/data/stacks/*/stack_*_bf_*.fits")" = 3 ] && echo 1)"

# 1: video only: the stacker is not asked
post /capture/after-burst '{"mode":"video"}' PUT >/dev/null; N0=$(st "d['after_burst']['stacked']"); bursts vo
ok "video: three full videos, nothing stacked from memory" "$([ "$(count "$T/data/*/*_vo_*.ser")" = 3 ] && [ "$(st "d['after_burst']['stacked']")" = "$N0" ] && echo 1)"

# the stacker dies in the middle of a stack-only recording: every burst is either stacked or written as video, and it is shown
post /capture/after-burst '{"mode":"stack_only"}' PUT >/dev/null
post /capture/bursts '{"name":"dead","frames":24,"interval_s":4,"bursts":5,"format":"ser"}' >/dev/null
for i in $(seq 1 40); do sleep 1; [ "$(st "d['after_burst']['stacked']")" -gt "$N0" ] 2>/dev/null && break; done; sleep 5
kill -9 $SP 2>/dev/null; wait_idle; sleep 1
V=$(count "$T/data/*/*_dead_*.ser"); K=$(python3 -c "import json; print(sum('_dead_' in e['source'] for e in json.load(open('$T/data/stacks/manifest.json'))))")
ok "stacker dies mid-recording: all 5 bursts are accounted for (stacked or video: $K stacked, $V videos)" "$([ $((K + V)) -ge 5 ] && echo 1)"
ok "stacker dies mid-recording: the user is told, and it stays until dismissed" "$([ "$(st "len([n for n in d['notices'] if not n['acked']])")" -ge 1 ] && echo 1)"
ok "the event log records it" "$(grep -rq stacker_disconnected $T/cfg/logs $T/data/logs 2>/dev/null && echo 1)"
post /notices/ack '{}' >/dev/null
ok "dismissing clears the notices" "$([ "$(st "len([n for n in d['notices'] if not n['acked']])")" = 0 ] && echo 1)"
R2=$(post /capture/bursts '{"name":"x","frames":10,"interval_s":3,"bursts":2,"format":"ser"}')
ok "with the stacker gone, a stack-only recording is refused up front" "$(echo "$R2" | grep -q "stacker is not running" && echo 1)"
echo "-- $(basename "$0"): $([ $FAIL = 0 ] && echo passed || echo FAILED)"; exit $FAIL
