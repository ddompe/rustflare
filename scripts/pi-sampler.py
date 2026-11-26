#!/usr/bin/env python3
# Run ON the Pi, in the background, to record what the machine and the app were doing all day: memory (dirty pages, direct reclaim),
# the capture drive, CPU per thread, temperature and throttling, kernel USB/disk messages, and the app's own state (camera fps, buffer,
# drops, Sun position, guiding, mount tracking and rate, auto-flip stage, stacking). One JSON line every 2 s to the Pi's SD card
# (not the capture drive, so it does not disturb what it measures). Usage: nohup python3 pi-sampler.py [hours] &
import json, os, ssl, subprocess, sys, time, urllib.request
A = "https://localhost:8443/api/status"; ctx = ssl._create_unverified_context()
out_dir = os.path.expanduser("~/rf-sampler"); os.makedirs(out_dir, exist_ok=True)
hours = float(sys.argv[1]) if len(sys.argv) > 1 else 36
pid = int(subprocess.check_output(["systemctl", "show", "-p", "MainPID", "--value", "rustflare"]).strip())
def kv(path):
    d = {}
    for l in open(path):
        p = l.replace(":", " ").split()
        if len(p) >= 2:
            try: d[p[0]] = int(p[1])
            except ValueError: pass
    return d
def threads():
    o = {}
    for t in os.listdir(f"/proc/{pid}/task"):
        try:
            s = open(f"/proc/{pid}/task/{t}/stat").read(); c = s[s.index("(") + 1:s.rindex(")")]; f = s[s.rindex(")") + 2:].split()
            o[c] = o.get(c, 0) + int(f[11]) + int(f[12])
        except Exception: pass
    return o
def sh(*a):
    try: return subprocess.run(a, capture_output=True, text=True, timeout=5).stdout.strip()
    except Exception as e: return str(e)
def app():
    try:
        with urllib.request.urlopen(A, context=ctx, timeout=5) as r: d = json.load(r)
    except Exception as e: return {"api_error": str(e)}
    c, cam, m, s, g, af, ab = d["capture"], d.get("camera") or {}, d.get("mount") or {}, d.get("sun") or {}, d.get("guiding") or {}, d.get("auto_flip") or {}, d.get("after_burst") or {}
    return {"state": c["state"], "fps": round(cam.get("fps", 0), 1), "queued": c["buffer_queued"], "dropped": c.get("dropped"), "disk_mb_s": c["disk_mb_s"], "burst": c.get("burst"),
            "sun": [round(s["pos"]["x"]), round(s["pos"]["y"]), round(s["radius"])] if s.get("pos") else None, "tracking": (d.get("tracking") or {}).get("lost"),
            "guide": g.get("state"), "held": g.get("held"), "mount_tracking": m.get("tracking"), "rate": m.get("rate"), "slewing": m.get("slewing"), "link_up": m.get("link_up"),
            "flip": af.get("stage"), "ab_mode": ab.get("mode"), "stacking": ab.get("stacking"), "stacked": ab.get("stacked"), "sun_held": d.get("sun_held"), "err": d.get("error"), "notices": len([n for n in d.get("notices", []) if not n.get("acked")])}
end = time.time() + hours * 3600; last_slow = 0; kmsg_since = time.strftime("%Y-%m-%d %H:%M:%S")
while time.time() < end:
    t = time.time(); m = kv("/proc/meminfo"); v = kv("/proc/vmstat")
    ds = next((l.split() for l in open("/proc/diskstats") if l.split()[2] == "sda"), None)
    row = {"t": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(t)), "avail_mb": m["MemAvailable"] // 1024, "dirty_mb": m["Dirty"] // 1024, "wb_mb": m["Writeback"] // 1024,
           "allocstall": sum(x for k, x in v.items() if k.startswith("allocstall")), "pgscan_direct": v.get("pgscan_direct", 0), "sda_w_sectors": int(ds[9]) if ds else None, "sda_inflight": int(ds[11]) if ds else None,
           "cpu": [int(x) for x in open("/proc/stat").readline().split()[1:9]], "load": open("/proc/loadavg").read().split()[0], "threads": threads(), "app": app()}
    if t - last_slow > 10:
        last_slow = t; row["throttled"] = sh("vcgencmd", "get_throttled"); row["temp"] = sh("vcgencmd", "measure_temp")
        k = sh("journalctl", "-k", "--since", kmsg_since, "--no-pager", "-q"); kmsg_since = time.strftime("%Y-%m-%d %H:%M:%S")
        bad = [l for l in k.splitlines() if any(w in l.lower() for w in ("usb", "error", "fail", "reset", "i/o", "fat-fs", "under-voltage", "throttl"))]
        if bad: row["kernel"] = bad[-8:]
    with open(f"{out_dir}/{time.strftime('%Y-%m-%d', time.gmtime(t))}.jsonl", "a") as f: f.write(json.dumps(row) + "\n")
    time.sleep(max(0, 2 - (time.time() - t)))
