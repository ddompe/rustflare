#!/usr/bin/env python3
# Run ON the Pi: one full-rate recording to the capture drive with a 2 Hz sampler of memory (dirty pages, direct reclaim), the drive
# (bytes written, I/O wait), CPU per thread of the service, throttling and temperature, plus the camera and buffer from the API.
# Writes /tmp/baseline-<label>.json and deletes the test recording. Usage: pi-io-baseline.py <label> <seconds> <buffer MB>
# See docs/project/stall-baseline-2026-11-23.md for what it showed. Keep the live view open (curl the MJPEG stream) to include the preview.
import json, os, ssl, subprocess, sys, threading, time, urllib.request
A = "https://localhost:8443/api"; ctx = ssl._create_unverified_context()
label, secs, buf = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
def call(method, path, body=None):
    req = urllib.request.Request(A + path, method=method, data=json.dumps(body).encode() if body is not None else None, headers={"content-type": "application/json"})
    try:
        with urllib.request.urlopen(req, context=ctx, timeout=30) as r: return json.loads(r.read() or b"{}")
    except urllib.error.HTTPError as e: return {"http_error": e.code, "body": e.read().decode()[:200]}
def kv(path, sep=None):
    d = {}
    for l in open(path):
        p = l.replace(":", " ").split()
        if len(p) >= 2:
            try: d[p[0]] = int(p[1])
            except ValueError: pass
    return d
pid = int(subprocess.check_output(["pgrep", "-x", "rustflare-serve"]).split()[0]) if subprocess.run(["pgrep", "-x", "rustflare-serve"], capture_output=True).returncode == 0 else int(subprocess.check_output(["systemctl", "show", "-p", "MainPID", "--value", "rustflare"]).strip())
def threads():
    out = {}
    for t in os.listdir(f"/proc/{pid}/task"):
        try:
            s = open(f"/proc/{pid}/task/{t}/stat").read(); comm = s[s.index("(") + 1:s.rindex(")")]; f = s[s.rindex(")") + 2:].split()
            out[t] = (comm, int(f[11]) + int(f[12]))
        except Exception: pass
    return out
samples, stop = [], threading.Event()
def sampler():
    last_api = 0
    while not stop.is_set():
        t = time.time(); m = kv("/proc/meminfo"); v = kv("/proc/vmstat")
        ds = next((l.split() for l in open("/proc/diskstats") if l.split()[2] == "sda"), None)
        cpu = [int(x) for x in open("/proc/stat").readline().split()[1:9]]
        s = {"t": round(t, 2), "avail_mb": m["MemAvailable"] // 1024, "dirty_mb": m["Dirty"] // 1024, "writeback_mb": m["Writeback"] // 1024, "cached_mb": m["Cached"] // 1024,
             "allocstall": sum(x for k, x in v.items() if k.startswith("allocstall")), "pgscan_direct": v.get("pgscan_direct", 0), "pgsteal_direct": v.get("pgsteal_direct", 0),
             "sda_sectors_w": int(ds[9]) if ds else 0, "sda_inflight": int(ds[11]) if ds else 0, "sda_io_ms": int(ds[12]) if ds else 0, "cpu": cpu, "threads": threads()}
        if t - last_api >= 1:
            last_api = t
            try:
                st = call("GET", "/status"); c = st["capture"]
                s.update({"fps": round(st["camera"]["fps"], 1), "queued": c["buffer_queued"], "dropped": c.get("dropped"), "disk_mb_s": c["disk_mb_s"], "state": c["state"]})
            except Exception as e: s["api_error"] = str(e)
            try: s["throttled"] = subprocess.run(["vcgencmd", "get_throttled"], capture_output=True, text=True).stdout.strip(); s["temp"] = subprocess.run(["vcgencmd", "measure_temp"], capture_output=True, text=True).stdout.strip()
            except Exception: pass
        samples.append(s); time.sleep(max(0, 0.5 - (time.time() - t)))
th = threading.Thread(target=sampler); th.start()
time.sleep(3)
call("PUT", "/capture/buffer", {"mb": buf})
t0 = time.strftime("%Y-%m-%dT%H:%M:%S", time.gmtime()); name = "baseline" + label
r = call("POST", "/capture/sequence", {"name": name, "seconds": secs, "format": "ser", "on_full": "drop"}); assert "http_error" not in r, r
t_start = time.time(); time.sleep(5)
while time.time() < t_start + secs + 600:
    time.sleep(1)
    if call("GET", "/status")["capture"]["state"] == "idle": break
t_end = time.time(); time.sleep(5); stop.set(); th.join()
ev = [e for e in call("GET", "/debug/events?limit=2000") if e["event"] in ("frame_gap", "capture_stopped_early", "frames_dropped") and e["t"][:19] >= t0]
day = time.strftime("%Y-%m-%d", time.gmtime())
lst = call("GET", "/files/list?path=" + day)["entries"]
mine = [e for e in lst if name in e["name"]]
frames = sum(call("GET", "/files/stats?path=" + e["path"]).get("frames_present", 0) for e in mine if e["name"].endswith(".ser"))
for e in mine: call("POST", "/files/delete", {"path": e["path"], "confirm": True})
json.dump({"label": label, "secs": secs, "buffer_mb": buf, "version": subprocess.check_output(["dpkg-query", "-W", "-f", "${Version}", "rustflare"]).decode(), "t_start": t_start, "t_end": t_end, "frames_written": frames, "events": ev, "samples": samples}, open(f"/tmp/baseline-{label}.json", "w"))
print("done", label, frames, len(samples), len(ev))
