#!/usr/bin/env bash
# Print a one-screen summary of a running RustFlare server (read-only).
#   scripts/pi-status.sh [http://astropi.local:8080]
URL="${1:-http://astropi.local:8080}"
curl -s "$URL/api/status" | python3 -c '
import sys, json
s = json.load(sys.stdin)
c = s["camera"]
if c:
    ctl = {k["id"]: k["value"] for k in c["controls"] if k["id"] in ("exposure_us", "gain", "offset", "high_bit_depth")}
    print("camera : %s  %dx%d %d-bit  %.1f fps  %s" % (c["name"], c["width"], c["height"], c["bits"], c["fps"], ctl))
else:
    print("camera : (none open)")
m = s["mount"]
print("mount  : %s  tracking=%s" % (m["info"], m["tracking"]) if m else "mount  : (none connected)")
t = s["tracking"]; g = s["guiding"]; k = s["capture"]
print("track  : %s | guiding=%s calibrated=%s | capture=%s %s" % (t["mode"] if t else None, g["state"], bool(g["calibration"]), k["state"], k["phase"]))
o = s["optics"]
print("optics : focal=%s x%s -> %s mm, measured radius=%s, suggested factor=%s" % (o["focal_length_mm"], o["focal_multiplier"], o["effective_focal_mm"], o.get("measured_radius_px"), o.get("suggested_multiplier")))
print("error  : %s" % s["error"])
'
