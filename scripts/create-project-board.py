#!/usr/bin/env python3
"""One-off: create the GitHub Project board, labels and issues that mirror docs/project/roadmap.md.

Needs `gh` logged in with the `project` scope (gh auth refresh -h github.com -s project).
Re-running creates duplicates; it is meant to be run once.
"""
import json
import subprocess
import sys

OWNER, REPO = "ddompe", "ddompe/rustflare"
TITLE = "Rustflare roadmap"


def gh(*args, check=True):
    r = subprocess.run(["gh", *args], capture_output=True, text=True)
    if check and r.returncode != 0:
        sys.exit(f"gh {' '.join(args[:3])} failed: {r.stderr.strip()}")
    return r.stdout.strip()


LABELS = {
    "hardware-verification": ("d93f0b", "Needs the real camera, mount, drive or telescope"),
    "feature": ("0e8a16", "New capability"),
    "docs": ("0075ca", "Documentation"),
    "infra": ("5319e7", "Packaging, CI, releases"),
    "tracking": ("fbca04", "Tracking, calibration, guiding"),
    "ui": ("c5def5", "Web UI"),
    "decision": ("b60205", "Needs a decision from the maintainer"),
}

# (done?, labels, title, body)
ITEMS = [
    # ------------------------------------------------------------------ pending
    (False, ["hardware-verification"], "Verify the Player One camera on real hardware",
     "Enumerate/open, exposure and gain ranges, real frame rate, the udev rule (the USB vendor id `a0a0` is unverified: compare with `lsusb`), colour/mono handling.\n\nSee docs/reference/hardware.md and ADR 0006."),
    (False, ["hardware-verification"], "Verify the ZWO AM3 on real hardware",
     "Model reply to `:GVP#`, solar rate `:TS#`, slew and guide directions, pulse behaviour, calibration result, guiding quality; equatorial vs alt-az mode.\n\nSee docs/reference/hardware.md, docs/reference/tracking-and-guiding.md and ADR 0007."),
    (False, ["hardware-verification", "tracking"], "Verify tracking on real H-alpha images",
     "Otsu sky/disc separation, limb fit with a partial disc and the Optics radius, anchor on a sunspot/filament/plage, behaviour with prominences.\n\nSee ADR 0013."),
    (False, ["hardware-verification"], "Verify USB-drive capture on the FAT32 stick",
     "Select the drive from the Storage panel, record, check SER `_partN` rollover at 4 GB, and unplug it mid-capture (must stop cleanly and never write to the SD card).\n\nSee ADR 0011."),
    (False, ["hardware-verification"], "Verify RAM buffer and bursts under a real high-fps load",
     "Real camera at its maximum frame rate: buffer fill, disk MB/s, back-pressure behaviour, scheduled bursts (\"100 frames every X s\"), CPU on the Pi.\n\nSee ADR 0012."),
    (False, ["feature"], "Guided flats workflow",
     "Flats start instantly today. Planned: step-by-step dialog: put the flat cap/diffuser on (live view + histogram) -> adjust gain/exposure (suggested exposure, clipping indicator, tracking paused) -> **OK** captures N flats -> summary, previous settings restored, reminder to remove the cap. Also decide on darks/dark flats and naming that ties flats to the lights' settings.\n\nDeferred until the base setup is stable on real hardware."),
    (False, ["docs"], "User guide",
     "Task-oriented guide for observers in `docs/guide/`: setting up at the scope; getting the Sun in view and centred; an H-alpha session from first light to files; planning bursts and storage; flats; reading the screen (overlays, histogram, status); troubleshooting and FAQ, with screenshots from real sessions. Link it from the README and the UI."),
    (False, ["infra", "ui"], "Playwright UI tests in CI (+ opt-in real-Pi profile)",
     "Browser-level tests against the simulator (open camera, track, calibrate, capture) with screenshots uploaded as CI artifacts. An opt-in profile can run the same checks against a real Pi; it must never move the mount without an explicit flag."),
    (False, ["ui"], "Visual review of the newer UI panels",
     "Display controls, Storage, Optics, bursts/buffer were type-checked and served but not reviewed visually after the first layout. Check layout on desktop and phone, and the slider value labels."),
    (False, ["decision", "infra"], "Publish the first release (v0.2.0): decide on redistributing Player One's library",
     "The `.deb` bundles Player One's proprietary `libPlayerOneCamera` (via the `playerone-sdk-sys` crate). Confirm Player One's terms (or ask them) before publishing release assets, then `git tag v0.2.0 && git push origin v0.2.0` (see docs/project/development.md). Alternative: ship a package without the library and download it at install time.\n\nSee ADR 0006 and ADR 0010."),
    (False, ["infra"], "Signed apt repository (GitHub Pages)",
     "So `apt upgrade` works without `rustflare-update` and updates are signature-verified (SHA-256 alone does not protect against a compromised release). Needs a signing key owned by the maintainer.\n\nSee ADR 0010."),
    (False, ["feature"], "Streaming capture with the SDK's stream mode",
     "The current capture does one start/stop-exposure call per frame, which limits the frame rate. Use the Player One SDK streaming mode for higher rates."),
    (False, ["feature"], "Drop-frames policy for a full RAM buffer; FITS output; dark frames",
     "Optional policy for lucky imaging where timing matters more than completeness; FITS as an output format; dark frames as a capture type."),
    (False, ["feature"], "ROI, binning and cooler/temperature controls; colour (Bayer) cameras",
     "Camera controls beyond exposure/gain/offset/bit depth. Colour sensors currently deliver raw Bayer treated as mono."),
    (False, ["feature", "infra"], "Authentication and HTTPS",
     "The server has no authentication and can move a mount. Add a token (and optionally HTTPS) before exposing beyond a trusted network."),
    (False, ["tracking"], "Configurable guiding parameters; refreshable anchor template",
     "Guide margin, speed and intervals are fixed in code today; the anchor reference block is never refreshed."),
    (False, ["feature"], "More drivers: ZWO ASI, V4L2 cameras; ASCOM/INDI bridges; AM3/AM5 position readout and GoTo", ""),
    # --------------------------------------------------------------------- done
    (True, ["feature"], "Rust workspace, hardware abstraction and simulators",
     "Crates for core types/traits, cameras, mounts, tracking, capture and the server; simulated camera and mount for testing without hardware. ADR 0002, 0005."),
    (True, ["ui"], "Web UI (React + Vite + TypeScript)",
     "Live MJPEG view, controls, status over WebSocket, served by the Rust binary. ADR 0003, 0004."),
    (True, ["tracking"], "Tracking: limb (partial disc), feature (anchor), disc; H-alpha-robust detection; Optics",
     "Otsu threshold on a log histogram, limb circle fit with a locked radius, optics -> expected radius. ADR 0008, 0013."),
    (True, ["tracking"], "Automatic calibration, guiding, Centre & guide / Hold position",
     "Calibration pulses each axis and measures the image motion; guider keeps the target; closed-loop tests. ADR 0008."),
    (True, ["feature"], "Capture: single frames, sequences, scheduled bursts, RAM buffer, FAT32 rollover",
     "Asynchronous writer thread, SER or PNG output, back-pressure instead of drops. ADR 0009, 0012."),
    (True, ["feature"], "Flats (basic): automatic exposure, SER + master flat",
     "Works but starts immediately; a guided workflow is tracked separately. ADR 0009."),
    (True, ["feature"], "Storage selection and safety",
     "Choose a USB drive in the UI, free-space and FAT32 handling, never silently write to the SD card. ADR 0011."),
    (True, ["ui"], "Display controls and centring aids",
     "Bullseye rings, crosshair, Sun outline and offset arrow, zoom/pan, invert, H-alpha tint, brightness/contrast/gamma, auto-stretch, histogram, preview fps cap."),
    (True, ["infra"], "Debian package, rustflare-update, CI and release workflows",
     "Cross-compiled arm64 .deb, systemd service, udev rule; install, upgrade and update verified on a Raspberry Pi 5; CI (tests, smoke, package) passing. ADR 0010."),
    (True, ["docs"], "Documentation and ADRs",
     "docs/ (MkDocs-ready) and 13 architecture decision records."),
]


def main():
    existing = {l["name"] for l in json.loads(gh("label", "list", "--repo", REPO, "--json", "name", "--limit", "100"))}
    for name, (color, desc) in LABELS.items():
        if name not in existing:
            gh("label", "create", name, "--repo", REPO, "--color", color, "--description", desc)

    proj = json.loads(gh("project", "create", "--owner", OWNER, "--title", TITLE, "--format", "json"))
    num, pid = str(proj["number"]), proj["id"]
    gh("project", "link", num, "--owner", OWNER, "--repo", REPO)
    gh("project", "edit", num, "--owner", OWNER, "--description", "Plan and progress for Rustflare. Mirrors docs/project/roadmap.md.")

    fields = json.loads(gh("project", "field-list", num, "--owner", OWNER, "--format", "json"))["fields"]
    status = next(f for f in fields if f["name"] == "Status")
    opt = {o["name"]: o["id"] for o in status["options"]}
    todo, done = opt["Todo"], opt["Done"]

    for is_done, labels, title, body in ITEMS:
        args = ["issue", "create", "--repo", REPO, "--title", title, "--body", body or title]
        for l in labels:
            args += ["--label", l]
        url = gh(*args).splitlines()[-1]
        item = json.loads(gh("project", "item-add", num, "--owner", OWNER, "--url", url, "--format", "json"))
        gh("project", "item-edit", "--id", item["id"], "--project-id", pid,
           "--field-id", status["id"], "--single-select-option-id", done if is_done else todo)
        if is_done:
            gh("issue", "close", url, "--repo", REPO, "--comment", "Completed; see docs/project/roadmap.md.")
        print(("done " if is_done else "todo ") + title)
    print(f"\nBoard: https://github.com/users/{OWNER}/projects/{num}")


if __name__ == "__main__":
    main()
