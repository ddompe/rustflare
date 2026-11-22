#!/usr/bin/env python3
"""Summarise a field session from its logs (events.jsonl, mount.jsonl, session_*.jsonl) as Markdown.

    scripts/analysis/field_report.py <folder with the logs, searched recursively> [--day YYYY-MM-DD]

Copy the logs off the Pi first: /var/lib/rustflare/logs (events, mount commands) and <drive>/rustflare/logs (session logs).
Standard library only. Nothing identifying is printed (the logs hold none).
"""
import argparse, collections, glob, json, math, os, statistics, sys, time
from datetime import datetime, timezone

ap = argparse.ArgumentParser()
ap.add_argument("folder")
ap.add_argument("--day", default=None, help="only this UTC day")
a = ap.parse_args()

def lines(pattern):
    for p in sorted(glob.glob(os.path.join(a.folder, "**", pattern), recursive=True)):
        for l in open(p, errors="replace"):
            try: yield p, json.loads(l)
            except Exception: pass

def ts(s):  # "2026-11-22T17:41:10.635Z" -> epoch
    return datetime.fromisoformat(s.replace("Z", "+00:00")).timestamp()
def hm(t): return time.strftime("%H:%M:%S", time.gmtime(t))
def pct(v, q): v = sorted(v); return v[min(len(v) - 1, int(q * len(v)))] if v else float("nan")
def sel(day, t): return a.day is None or time.strftime("%Y-%m-%d", time.gmtime(t)) == a.day

out = []
def h(s): out.append("\n## " + s + "\n")
def p(s=""): out.append(s)

# ---------------- events
ev = {}
for _, e in lines("events.jsonl"):
    try: t = ts(e["t"])
    except Exception: continue
    if sel(a.day, t): ev[(t, e["event"], json.dumps(e.get("detail"), sort_keys=True))] = (t, e)
events = [v for _, v in sorted(ev.items())]
h("Events")
if events:
    p(f"{len(events)} events from {hm(events[0][0])} to {hm(events[-1][0])} UTC.")
    c = collections.Counter(e["event"] for _, e in events)
    p("")
    p("| event | count |"); p("|---|---|")
    for k, n in c.most_common(): p(f"| {k} | {n} |")
    # loss episodes
    lost = None; spans = []
    for t, e in events:
        if e["event"] == "tracking_lost" and lost is None: lost = t
        if e["event"] == "tracking_found" and lost is not None: spans.append((lost, t)); lost = None
    if spans:
        d = [b - a_ for a_, b in spans]
        p(f"\nTracking lost {len(spans)} times; duration median {statistics.median(d):.1f} s, p90 {pct(d, .9):.1f} s, longest {max(d):.0f} s.")
    cal = [(t, e["detail"]) for t, e in events if e["event"] == "calibration_done"]
    if cal:
        p("\n**Calibrations**\n"); p("| time (UTC) | rate primary / secondary (px/s at 1x) | primary direction | secondary direction |"); p("|---|---|---|---|")
        for t, d in cal:
            try:
                ang = lambda v: math.degrees(math.atan2(v["y"], v["x"])) % 360
                p(f"| {hm(t)} | {d['rate'][0]:.1f} / {d['rate'][1]:.1f} | {ang(d['primary_dir']):.0f}° | {ang(d['secondary_dir']):.0f}° |")
            except Exception: pass
    for name in ("dead_reckoning_start", "dead_reckoning_gave_up", "capture_stopped_early", "session_log_stopped", "flip_done", "auto_flip_record_failed", "capture_abort"):
        xs = [(t, e) for t, e in events if e["event"] == name]
        if xs: p(f"\n`{name}`: " + "; ".join(f"{hm(t)} {json.dumps(e['detail'])[:90]}" for t, e in xs[:6]))
else:
    p("none found")

# ---------------- mount commands
h("Mount command log")
mc = {}
for _, m in lines("mount.jsonl"):
    try: t = ts(m["t"])
    except Exception: continue
    if sel(a.day, t): mc[(t, m["cmd"])] = (t, m)
mounts = [v for _, v in sorted(mc.items())]
if mounts:
    cmds = collections.Counter(m["cmd"] for _, m in mounts)
    lat = [m["ms"] for _, m in mounts if "ms" in m]
    errs = [(t, m) for t, m in mounts if "error" in m]
    p(f"{len(mounts)} commands from {hm(mounts[0][0])} to {hm(mounts[-1][0])} UTC (unique by time and text). Round trip: median {statistics.median(lat):.0f} ms, p95 {pct(lat, .95):.0f} ms, max {max(lat)} ms. Errors: {len(errs)}.")
    p("\n| command | count |"); p("|---|---|")
    for k, n in cmds.most_common(12): p(f"| `{k}` | {n} |")
    pulses = [(t, m["cmd"]) for t, m in mounts if m["cmd"].startswith(":Mg") and m["cmd"].endswith("3000#")]
    if pulses:
        hours = max(1e-9, (mounts[-1][0] - mounts[0][0]) / 3600)
        p(f"\nGuide commands (3 s timed pulses, chained for long corrections): {len(pulses)} in {hours:.1f} h, {len(pulses) / hours:.0f} per hour.")
    gu = [(t, m.get("reply", "")) for t, m in mounts if m["cmd"] == ":GU#"]
    flips = []; prev = None
    for t, r in gu:
        trk = not r.startswith("n") if r else None
        if prev is not None and trk != prev: flips.append((t, trk))
        prev = trk
    if flips: p("\nMount tracking flag changes (from `:GU#`): " + ", ".join(f"{hm(t)} {'on' if on else 'OFF'}" for t, on in flips[:14]))
    if errs: p("\nErrors: " + "; ".join(f"{hm(t)} `{m['cmd']}` {m['error'][:60]}" for t, m in errs[:6]))
else:
    p("none found")

# ---------------- session logs
h("Session logs (tracking and guiding, once a second)")
rows = []
for path, d in lines("session_*.jsonl"):
    try: t = d["t_ms"] / 1000
    except Exception: continue
    if sel(a.day, t): rows.append((t, d))
rows.sort(key=lambda x: x[0])
seen = set(); uniq = []
for t, d in rows:
    k = int(t * 10)
    if k in seen: continue
    seen.add(k); uniq.append((t, d))
rows = uniq
if rows:
    p(f"{len(rows)} samples from {hm(rows[0][0])} to {hm(rows[-1][0])} UTC.")
    g = [d["guide"]["error_px"] for t, d in rows if d.get("guide", {}).get("state") == "guiding" and d["guide"].get("error_px") is not None]
    if g: p(f"\nGuiding error while guiding ({len(g)} samples): median {statistics.median(g):.1f} px, p90 {pct(g, .9):.1f}, p99 {pct(g, .99):.1f}, max {max(g):.0f}.")
    # hourly split
    byh = collections.defaultdict(list)
    for t, d in rows:
        if d.get("guide", {}).get("state") == "guiding" and d["guide"].get("error_px") is not None:
            byh[time.strftime("%H:00", time.gmtime(t))].append(d["guide"]["error_px"])
    if byh:
        p("\n| hour (UTC) | samples | median error | p90 | max |"); p("|---|---|---|---|---|")
        for k in sorted(byh): v = byh[k]; p(f"| {k} | {len(v)} | {statistics.median(v):.1f} | {pct(v, .9):.1f} | {max(v):.0f} |")
    lv = [d["cam"]["mean"] for t, d in rows if "cam" in d]
    if lv: p(f"\nLight level (mean frame brightness 0..1): median {statistics.median(lv):.2f}, p5 {pct(lv, .05):.3f}, min {min(lv):.3f}; {sum(1 for x in lv if x < .3)} samples under 0.30 (cloud or shadow).")
    pl = sum(len(d.get("pulses") or []) for t, d in rows)
    p(f"\nGuide pulses logged: {pl}.")
    # slides: sun model steps
    steps = []; prev = None
    for t, d in rows:
        s = d.get("sun")
        if not s: continue
        if prev and t - prev[0] < 3 and math.hypot(s["x"] - prev[1]["x"], s["y"] - prev[1]["y"]) > 100: steps.append(t)
        prev = (t, s)
    byh2 = collections.Counter(time.strftime("%H:00", time.gmtime(t)) for t in steps)
    p(f"\nSun-model steps over 100 px between consecutive samples: {len(steps)} (per hour UTC: " + ", ".join(f"{k} {byh2[k]}" for k in sorted(byh2)) + ").")
else:
    p("none found")

print("# Field report\n" + "\n".join(out))
