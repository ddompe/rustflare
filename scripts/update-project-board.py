#!/usr/bin/env python3
"""Sync the GitHub board with the state recorded in docs/project/roadmap.md (run after a work session).
Edit the lists below, then run: python3 scripts/update-project-board.py"""
import json, subprocess, sys
REPO, OWNER, NUM = "ddompe/rustflare", "ddompe", "1"

def gh(*a):
    r = subprocess.run(["gh", *a], capture_output=True, text=True)
    if r.returncode: sys.exit(f"gh {' '.join(a[:3])}: {r.stderr.strip()}")
    return r.stdout.strip()

proj = json.loads(gh("project", "view", NUM, "--owner", OWNER, "--format", "json"))
fields = json.loads(gh("project", "field-list", NUM, "--owner", OWNER, "--format", "json"))["fields"]
st = next(f for f in fields if f["name"] == "Status")
opt = {o["name"]: o["id"] for o in st["options"]}

def new_issue(title, body, labels, done=False):
    args = ["issue", "create", "--repo", REPO, "--title", title, "--body", body]
    for l in labels: args += ["--label", l]
    url = gh(*args).splitlines()[-1]
    item = json.loads(gh("project", "item-add", NUM, "--owner", OWNER, "--url", url, "--format", "json"))
    gh("project", "item-edit", "--id", item["id"], "--project-id", proj["id"], "--field-id", st["id"],
       "--single-select-option-id", opt["Done" if done else "Todo"])
    if done: gh("issue", "close", url, "--repo", REPO, "--comment", "Completed; see docs/project/roadmap.md.")
    print(("done " if done else "todo ") + title)

def comment(n, text, close=False):
    gh("issue", "comment", str(n), "--repo", REPO, "--body", text)
    if close: gh("issue", "close", str(n), "--repo", REPO)
    print(("closed #" if close else "commented #") + str(n))

# ---- existing issues: results of the first real session
comment(1, "**Verified on the real camera** (Apollo-M MINI, Pi 5, USB 3): enumeration, opening, controls (exposure 10 us-2000 s, gain 0-550, offset, 8/16-bit), 1944x1472 frames, 4.5 um pixel size, temperature, USB vendor id `a0a0` and the udev rule. Frame rate: one-shot capture was a constant 8.4 fps; continuous mode gives 57 fps (16-bit) / 82 fps (8-bit).", close=True)
comment(2, "**Partly verified** with the real AM3 in a live session: connects over USB serial (model detected), solar tracking, automatic calibration and guiding ran. Still to verify: the new drift-correcting guider on the real sky, equatorial vs alt-az behaviour, and keeping tracking on across service restarts (new issue).")
comment(3, "**Verified offline on the real recordings** (limb tracking with the Optics radius): the located centre is stable to ~2 px (sigma) over a 100-frame burst; frame-to-frame step ~1.4-2 px. A free limb fit (no radius hint) is biased on this short arc: radius +11 %, centre ~140 px off (new issue for a geometric refinement). Still to check: the anchor tracker on a real sunspot/filament, and live tracking while the Sun moves.")
comment(4, "**Partly verified:** ten 572 MB bursts written to the FAT32 stick through the UI-selected location, byte-exact. Still to verify: the 4 GB SER split on the real stick (record about 800 frames in one sequence) and unplug behaviour.")
comment(5, "**Verified on the real camera:** ten 100-frame bursts at 57 fps, exactly 20 s apart, no frames lost, durable flush to the stick. Longer sustained runs are covered under the USB-drive issue.", close=True)
comment(9, "Folded into the UI redesign (side agent in progress); see the UI design review issue.", close=True)
comment(16, "The guider now has parameters (window, dead band, gain, speed, intervals) with defaults tuned from the real seeing (sigma ~2 px); exposing them in the UI is still open.")
comment(28, "**Started** (capture is stable enough): a side agent is redesigning the UI with the impeccable skill. Extra requirements given: responsive iPhone web app (Home Screen, safe areas, 44 pt targets), saved settings on the Pi, device cards with datasheets, the RustFlare brand.")
comment(29, "**Implemented** (unit-tested; not yet run on the real sky): the guider now averages the position over a 10 s window (seeing cancels), ignores offsets under a 3 px dead band, and sends small gentle pulses sized from the calibrated px/s response. Real-data seeing is ~2 px sigma, which supports these defaults. Keep open until verified on the real mount.")
comment(10, "Plan: the first release is **v0.3.0** (see docs/project/roadmap.md, Before v0.3.0). Still needs the decision on redistributing Player One's library.")

# ---- new work
new_issue("Support ZWO ASI cameras (the owner has one to try)",
 "Port Vidoxide's ASI driver (MIT, `ext/libasicamera-sys` + `src/camera/drivers/asi.rs`) to our `Camera` trait behind a Cargo feature. Same controls ids, continuous capture, pixel size. Test with the owner's ZWO camera on another day. Needs the ASI SDK library (not bundled upstream): decide how to ship it.", ["feature", "hardware-verification"])
new_issue("EAF (electronic automatic focuser) support + seeing-robust autofocus",
 "1) Drive a ZWO EAF (position, move, temperature) behind a `Focuser` trait, like `Mount`. 2) Autofocus that is resistant to turbulence: compute the focus metric on an **average of several frames** (aligned on the limb/feature) instead of single frames, so seeing does not corrupt the metric; evaluate on the recorded bursts offline first (single-frame metric scatter vs averaged). 3) UI: focus panel with a focus-quality readout and a V-curve.", ["feature", "tracking"])
new_issue("Device datasheets and inventory (ZWO and Player One planetary/solar cameras, ZWO AM3)",
 "Images and spec sheets shown in the Camera and Mount panels: sensor, resolution, pixel size, ADC bits, full well, read noise, **HCG point** (gain at which high conversion gain switches on), unity gain, max fps, cooling; mount: payload, modes, guide/tracking rates, ports. Build an inventory (catalog JSON, with source URLs) for ZWO and Player One planetary/solar cameras and the AM3, matched to the connected device; mark the HCG point on the gain control. Original illustrations only (no vendor photos), unless the owner supplies licensed ones. API contract is in the design agent's brief: `GET /api/datasheet/camera` and `/mount`.", ["feature", "ui", "docs"])
new_issue("Keep the mount's tracking on across service restarts",
 "Connecting to the ZWO mount sends `:Td#`, which switches sky tracking off, so a service restart or update stops the mount. Read the mount's tracking state on connect instead (`:GU#`, see INDIGO's driver) and leave it alone; verify on the real AM3 (read-only query first).", ["tracking", "hardware-verification"])
new_issue("Free limb fit is biased on short arcs: add a geometric refinement",
 "Found on the real recordings: with no radius hint the algebraic (Kasa) fit overestimates the radius (+11 %) and moves the centre ~140 px. Refine with a geometric Gauss-Newton fit on (cx, cy, r) after the Kasa start; validate on the recorded bursts (`crates/server/examples/analyze_ser.rs`).", ["tracking"])
new_issue("Real-data regression fixtures and analysis tooling",
 "Keep a few downsampled real H-alpha frames from the 2026-11-21 recordings as test fixtures (partial disc, 100-frame burst statistics) and extend `analyze_ser` (seeing statistics, guider replay). Tune guider defaults from real seeing.", ["tracking", "infra"])
new_issue("iPhone web app: responsive layout, Home Screen app",
 "The UI must work well as a Home Screen web app on a modern iPhone: portrait and landscape, safe areas, 44 pt touch targets, sunlight legibility, slew pad that always stops (pointercancel/blur), manifest + icons from the RustFlare mark. Plain HTTP on the LAN means no service worker or wake lock (needs HTTPS; see the HTTPS issue): tell users to set Auto-Lock to Never. Delivered through the UI redesign; test in WebKit with an iPhone profile.", ["ui"])
new_issue("Field networking: Pi hotspot / access-point mode",
 "Observing sites often have no Wi-Fi. Make the Pi easy to reach from a phone without existing infrastructure: run it as an access point (or join the phone's hotspot) with a stable address, documented and scripted.", ["infra", "feature"])
new_issue("Persist UI and camera settings across reboots", "Server-side settings store (`/api/ui`, `useSetting`), per-camera exposure/gain/offset/bit depth re-applied on open. Verified by restart tests.", ["feature", "ui"], done=True)
new_issue("Auto-reconnect camera and mount at startup", "Reconnects the last camera and mount (retrying while USB comes up); never switches mount tracking on by itself. Verified on the real camera and mount.", ["feature"], done=True)
new_issue("Drift-correcting guider (averaged, dead band, gentle pulses)", "Replaces the fixed 1 s pulse guider that overshot at fine pixel scales. Unit-tested (pure seeing produces zero pulses; drift is held). Real-sky verification is tracked in the guiding issue.", ["tracking"], done=True)
new_issue("Streaming (continuous) capture for the Player One driver", "57 fps instead of a constant 8.4 fps. Verified on the real camera.", ["feature"], done=True)
new_issue("RustFlare logo and brand notes", "Mark in the README and UI assets, brand colours in docs/project/brand.md.", ["docs", "ui"], done=True)
