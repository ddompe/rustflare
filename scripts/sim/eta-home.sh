#!/bin/bash
# The end-time estimate against the real finish, and "send the mount home when finished", for a repeated recording and for lanes.
R="$(cd "$(dirname "$0")/../.." && pwd)"; T="${TMPDIR:-/tmp}/rustflare-sim/eta-home"; rm -rf "$T"; mkdir -p "$T/data"
"$R/target/release/rustflare-server" --bind 127.0.0.1:18122 --web-dir "$R/web/dist" --data-dir "$T/data" --config-file "$T/c.json" >"$T/log" 2>&1 & PID=$!
trap 'kill $PID 2>/dev/null' EXIT; sleep 1.5
A=http://127.0.0.1:18122/api; J='content-type: application/json'
post(){ curl -s -X "$1" -H "$J" -d "${3:-{\}}" "$A$2"; echo; }
st(){ curl -s $A/status | python3 -c "import sys,json; s=json.load(sys.stdin); $1"; }
post POST /camera/open '{"driver":"simulator","id":"sim0"}' >/dev/null
post POST /mount/connect '{"driver":"simulator"}' >/dev/null
post POST /mount/tracking '{"enabled":true}' >/dev/null; sleep 2

run() { # name, path, body
  echo "== $1"
  post POST "$2" "$3" >/dev/null
  T0=$(date +%s); FIRST=""
  for i in $(seq 1 120); do
    sleep 2
    E=$(st 'c=s["capture"]; print(c["state"], "" if c["eta_s"] is None else round(c["eta_s"],1))')
    NOW=$(( $(date +%s) - T0 ))
    case "$E" in idle*) echo "   finished after ${NOW}s"; break;; esac
    [ -z "$FIRST" ] && { FIRST="$E"; ETA=${E#* }; echo "   first estimate at t=${NOW}s: ${ETA}s left -> predicts the end at about $(( NOW + ${ETA%.*} ))s"; }
  done
  sleep 6
  st 'm=s["mount"]; print("   mount at_home:", m["at_home"], "| tracking:", m["tracking"])'
}
run "repeated recording: 4 bursts of 1 s every 6 s, then home" /capture/bursts '{"name":"etab","seconds":1,"interval_s":6,"bursts":4,"format":"ser","home_when_done":true}'
post POST /mount/tracking '{"enabled":true}' >/dev/null; sleep 2
st 'print("   before the next run: at_home", s["mount"]["at_home"])'
run "lanes: 3 cycles of 2 lanes every 10 s, no homing" /capture/lanes '{"name":"etal","cycles":3,"cycle_interval_s":10,"format":"ser","lanes":[{"name":"a","exposure_us":1000,"frames":10},{"name":"b","exposure_us":5000,"frames":10}]}'
