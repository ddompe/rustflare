#!/usr/bin/env bash
# Exercises the mount state/home API against the SIMULATOR mount (never touches real hardware).
#   scripts/mount-api-test.sh [path-to-server-binary]
set -u
BIN="${1:-./target/release/rustflare-server}"
T="${TMPDIR:-/tmp}/rustflare-mount-test-$$"
mkdir -p "$T"
A=http://127.0.0.1:18096/api
"$BIN" --bind 127.0.0.1:18096 --data-dir "$T/data" --config-file "$T/cfg.json" >"$T/log" 2>&1 &
PID=$!
trap 'kill $PID 2>/dev/null; wait $PID 2>/dev/null; rm -rf "$T"' EXIT
sleep 1.5

call() { curl -s -X "$1" -H 'content-type: application/json' -d "${3:-{\}}" "$A$2"; echo; }
mount_state() {
  curl -s "$A/status" | python3 -c '
import sys, json
m = json.load(sys.stdin)["mount"]
print("tracking=%s park=%s slewing=%s" % (m["tracking"], m["park"], m["slewing"]))'
}
fail=0
expect() { # description, actual, needle
  case "$2" in *"$3"*) echo "  ok   $1";; *) echo "  FAIL $1: got $2"; fail=1;; esac
}

call POST /mount/connect '{"driver":"simulator"}' >/dev/null
call POST /mount/tracking '{"enabled":true}' >/dev/null
sleep 2.5
expect "tracking read back from the mount" "$(mount_state)" "tracking=True park=unparked"
expect "home needs explicit confirmation" "$(call POST /mount/home)" "confirm"
expect "home with confirmation is accepted" "$(call POST /mount/home '{"confirm":true}')" "{}"
sleep 2.5
expect "state after home (read back from the mount)" "$(mount_state)" "tracking=False"
expect "tracking can be switched on again" "$(call POST /mount/tracking '{"enabled":true}')" "{}"
sleep 2.5
expect "and is read back as on" "$(mount_state)" "tracking=True"
exit $fail
