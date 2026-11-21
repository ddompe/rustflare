#!/usr/bin/env bash
# End-to-end smoke test against the built-in simulators (no hardware needed).
# Usage: scripts/smoke.sh [path-to-server-binary] [data-dir]     starts a local server
#        SMOKE_URL=http://<pi-name>.local:8080 scripts/smoke.sh     tests an already running server
set -u
BIN="${1:-./target/release/rustflare-server}"
DATA="${2:-./data-smoke}"
PORT=18080
if [ -n "${SMOKE_URL:-}" ]; then
  BASE="$SMOKE_URL"; REMOTE=1
else
  BASE="http://127.0.0.1:$PORT"; REMOTE=0
  rm -rf "$DATA"
  "$BIN" --bind "127.0.0.1:$PORT" --data-dir "$DATA" >"$DATA.log" 2>&1 &
  PID=$!
  trap 'kill $PID 2>/dev/null; wait $PID 2>/dev/null' EXIT
  sleep 1.5
fi
A="$BASE/api"

call() { # method path [json]
  curl -s -X "$1" -H 'content-type: application/json' -d "${3:-{\}}" "$A$2"; echo
}
state() {
  curl -s "$A/status" | python3 -c '
import sys, json
s = json.load(sys.stdin)
c = s["camera"] or {}
t = s["tracking"]
sun = s.get("sun")
print("  sun=%s" % (None if not sun else "(%.0f,%.0f) r=%.0f partial=%s" % (sun["pos"]["x"], sun["pos"]["y"], sun["radius"], sun["partial"])))
print("  guiding=%s calibrated=%s err_px=%s tracking=%s capture=%s/%s error=%s fps=%.1f mean=%.2f" % (
    s["guiding"]["state"], bool(s["guiding"]["calibration"]), s["guiding"]["error_px"],
    (t or {}).get("mode"), s["capture"]["state"], s["capture"]["phase"], s["error"],
    c.get("fps", 0), c.get("mean", 0)))
'
}

echo "== cameras";            curl -s "$A/cameras"; echo
echo "== open camera";        call POST /camera/open '{"driver":"simulator","id":"sim0"}'
echo "== connect mount";      call POST /mount/connect '{"driver":"simulator"}'
echo "== mount tracking on";  call POST /mount/tracking '{"enabled":true}'
sleep 1
echo "== track sun disc";     call POST /tracking/start '{"mode":"sun_disc"}'
echo "== calibrate";          call POST /guiding/calibrate
sleep 12; state
echo "== knock the Sun off centre (slew primary 2s), then centre & guide"
call POST /guiding/stop
call POST /mount/slew '{"axis":"primary","speed":8}'; sleep 2
call POST /mount/slew '{"axis":"primary","speed":0}'; sleep 1; state
call POST /guiding/start '{"center":true}'
sleep 25; state
echo "== single frame";       call POST /capture/single '{"name":"smoke"}'
sleep 2; state
echo "== sequence (30 frames)"; call POST /capture/sequence '{"name":"smoke","frames":30}'
sleep 6; state
echo "== scheduled bursts: 3 x 20 frames every 4 s (RAM buffer -> writer thread)"
call POST /capture/bursts '{"name":"smoke","frames":20,"interval_s":4,"bursts":3}'
sleep 2; curl -s "$A/status" | python3 -c 'import sys,json; c=json.load(sys.stdin)["capture"]; print("  mid-run: state=%s burst=%s/%s %s buffer=%s/%s disk=%.0f MB/s" % (c["state"],c["burst"],c["bursts_total"],c["phase"],c["buffer_queued"],c["buffer_capacity"],c["disk_mb_s"]))'
sleep 14; state
echo "== PNG-folder burst (10 frames)"
call POST /capture/sequence '{"name":"smokepng","frames":10,"format":"png"}'
sleep 6; state
echo "== flats: switch sim to flat scene, stop tracking"
call POST /tracking/stop
call PUT /camera/control '{"id":"sim_scene","value":1}'
call PUT /camera/control '{"id":"exposure_us","value":300}'
call POST /capture/flats '{"name":"smoke","frames":10,"level":0.5}'
sleep 8; state
echo "== error handling (expect 400s)"
call POST /capture/flats '{"frames":0}'
call POST /guiding/start
echo "== preview"
curl -s -o /dev/null -w "snapshot: HTTP %{http_code}, %{size_download} bytes\n" "$BASE/snapshot.jpg"
echo "== files"
curl -s "$A/files" | python3 -c 'import sys,json; [print("  %s (%d bytes)" % (f["path"], f["size"])) for f in json.load(sys.stdin)[:12]]'
if [ "$REMOTE" = 0 ]; then
  echo "== server log (warnings/errors)"
  grep -iE "warn|error|panic" "$DATA.log" || echo "  none"
fi
