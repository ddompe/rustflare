# Roadmap & status

Work is tracked on the [GitHub project board](https://github.com/users/ddompe/projects/1) (one issue per item; closed = done). This page is the narrative summary.

**Where we are:** version 0.8.0 (2026-11-27). The recorder, mount, tracker and guider have run on the Sun with the owner's Player One Apollo-M MINI, ZWO AM3 and Lunt LS50 on a Raspberry Pi 5; the stacker (`rustflare-stack`) stacks finished recordings from disk and, new in 0.5.0, bursts straight from the recorder's shared frame ring ([After each burst](../guide/recording-modes.md), [ADR 0019](../adr/0019-shared-frame-ring.md)). What still needs real equipment is on the [hardware checklist](hardware-checklist.md) and the issues labelled `hardware-verification`.

**Next, in rough order:** stacking from memory on the Sun for an hour ([#87](https://github.com/ddompe/rustflare/issues/87)); the stacker compared with AutoStakkert and Siril ([#91](https://github.com/ddompe/rustflare/issues/91)); the rest of the UI critique ([#88](https://github.com/ddompe/rustflare/issues/88)); the ZWO ASI camera and colour cameras (#30, #56, #92); the Dec slide (#82) and the stick's stalls (#86).

## Verified on the real system (first session, 2026-11-21)

Setup: Raspberry Pi 5 (Debian 13), Player One Apollo-M MINI (USB 3), ZWO AM3, H-alpha scope showing part of the disc (350 mm × 3 Barlow), FAT32 USB stick.

- **Camera:** enumeration, opening, controls, 1944×1472 16-bit frames, pixel size and temperature readout; the USB vendor id `a0a0` and the udev rule. **57 fps at 16-bit / 82 fps at 8-bit** in continuous mode (the one-shot mode gave a constant 8.4 fps).
- **Mount:** the AM3 connects over USB serial, solar tracking works, automatic calibration and guiding ran. The app now **reads the mount's real state** (tracking, slewing, at home, park, alt-az) every 2 s instead of assuming it: verified read-only on the real AM3 after the owner sent it home with its button (it reported tracking off, at home, equatorial mode).
- **Tracking on real H-alpha frames** (offline, from the recordings): with the Optics radius the located limb centre is stable to ~2 px (σ) over a 100-frame burst, which is the seeing; frame-to-frame step ≈ 1.4–2 px mean. **The Sun's radius in the image is ≈ 1227 px, not the 1099 px the entered optics predict** (350 mm × a nominal 3× Barlow): the limb points fit the free circle to 3 px RMS but the optics circle to 21 px. So the effective focal length is ≈ 1170 mm (a real Barlow factor ≈ 3.35×). The tracker now measures the radius from the limb whenever the optics value does not fit the data, and the Optics panel suggests the corrected focal length / factor.
- **Saved camera settings:** after a service restart the camera comes back with its exposure/gain (6000 µs / gain 71).
- **Bursts:** ten 100-frame bursts exactly 20 s apart, each file byte-exact (no frames lost), written to the FAT32 stick; auto-reconnect of camera and mount after a service restart; settings and camera exposure/gain persisted across restarts.
- **Packaging on the Pi:** install and upgrade of the `.deb`, config and captures preserved.
- **CI:** tests, simulator smoke test and the arm64 package build pass on GitHub.

## Since the first release

Released: 0.3.0 (first release), 0.3.1, 0.4.0 (field fixes from the first day on the Sun: brightness-invariant detection, one Sun model, GoTo to the Moon and planets, the stacker), 0.5.0 (frame ring, stacking from memory, notices), 0.6.0 (file manager with folders, stacks and folder download; prominence lanes saved raw with the black disc drawn after stacking; Re-stack; file formats reference), 0.8.0 (use profiles for the Sun, Moon and planets with a colour histogram and a planet locator; optical setups that carry their profile; ASI cooler and HCG marks; the mount's own site and time read back and sent (by itself after connecting); iOptron HAE69C verified on a real mount: slews, home, search home, tracking, site and time; the software version on every screen), 0.7.1 (ZWO ASI region of interest, Find my mount: the serial ports are asked which mount answers), 0.7.0 (iOptron mount driver, from the protocol document, mock-tested only; the view never scrolls: the Start strip is always on screen; Bluetooth scan picker), 0.6.5 (the stacker restart rule is written for the account the recorder runs as), 0.6.4 (UI critique: one Start, Flats and Darks on Record, stacker restarts itself, device photos), 0.6.3 (the stacker's main settings on the Record tab, stacks looked at in Files only), 0.6.2 (recording to the SD card with a reserve, a darkness guard for the tracker, the automatic flip refusing to flip a flipped mount, a USB 3 drive warning, the mount's own tracking reasons, internal and external stacking measured on the Sun; see [what to test next](next-sunny-day.md)), 0.6.1 (focus assist on the disc interior with an error band, write-behind SER writer, thumbnails and previews in Files, visible UI errors, drive-trouble messages). The first-release checklist (SER files in independent tools, the drift-correcting guider on the real sky, the FAT32 split and unplug behaviour, the redesigned UI on the phone, and the decision to publish) was worked through and closed on the board.

## Planned

- **ZWO ASI camera follow-ups:** the driver exists (ADR 0014; ASI174MM Mini enumerates, opens and streams on the Pi, USB 2 only so far). Still to do: measure on a USB 3 port, ROI/binning (frame rate), colour ASI cameras, cooler/fan controls.
- **Focus assist ([#78](https://github.com/ddompe/rustflare/issues/78))**: no focuser needed. A person turns the focuser while a short burst is scored per frame and as a lucky-imaging stack, shown as a live score, trend and best-so-far, so focus can be judged on a bright day. The same metric later drives the EAF autofocus.
- **EAF support and seeing-robust autofocus**: drive a ZWO EAF; compute a focus metric on an *average of several frames* (aligned on the limb) so turbulence does not corrupt the metric.
- **Colour (OSC) cameras**: Bayer frame formats, debayering for preview and stills, raw Bayer SER recording, tracking on a luminance/green image, colour UI (white balance, per-channel histogram).
- **Device datasheets and inventory**: images and spec sheets (including the HCG point) for ZWO and Player One planetary/solar cameras and the AM3, shown in the Camera and Mount panels.
- **Software coronagraph for prominences** (built in 0.4.0, see the prominence lanes): a second, longer exposure recorded after the regular one (disc saturated on purpose), with disc masks from the fitted limb, stacked separately and blended in post-processing; plus a live coronagraph preview (disc blacked out, rest stretched).
- **Send-home button** (`POST /api/mount/home`, implemented; to be verified on the mount) and a UI for the mount's real state. Connecting no longer switches tracking off; to be verified with the mount actually tracking.
- **iPhone web app**: responsive layout, Home Screen app (part of the UI redesign); HTTPS later enables a service worker and wake lock.
- **Field networking**: Pi hotspot / access-point mode for use away from Wi-Fi.
- **Guided flats** (cap-on, adjust, OK, capture, restore); darks; FITS output.
- **Browser-level UI tests** (Playwright, Chromium and WebKit) in CI, with an opt-in profile for the real Pi.
- Signed apt repository; authentication and HTTPS; configurable guiding parameters; ROI/binning/cooler controls; colour cameras; more drivers.

- **Bluetooth mount link** (issue #53): implemented (ADR 0015); needs a first live session with the mount in pairing mode to verify scanning, the session opener, guiding and slewing over BLE.

## Known limitations

- Mono only (colour cameras deliver Bayer, treated as mono).
- The day-of-year solar radius model is good to ≈ 0.1 %: fine for tracking, not for astrometry.
- Limb fitting needs some limb in view; thresholds assume the sky is darker than the disc.
- No authentication; keep the Pi on a trusted network.
- The SER header's local-time field carries UTC.
- Released packages redistribute Player One's proprietary library (see [ADR 0006](../adr/0006-playerone-via-vendor-sdk.md), [0010](../adr/0010-debian-package-and-updates.md)).


## Milestones (GitHub)

- **M1 Hardware-verified field use**: Go to Sun, Bluetooth and USB mount links, USB 3 capture path (done and verified); safe eject, USB recovery, flats, guider on a real sky, H-alpha tracking (built; see [hardware-checklist.md](hardware-checklist.md)).
- **M2 UI for the telescope**: overlays, one control scale, phone layout, file manager, user guide, datasheets (built); real iPhone check and the second UI review (open).
- **M3 More gear** (last): ASI174MM, colour cameras, EAF/autofocus.
- **M4 Workflows and release**: flats (done), coronagraph, HTTPS/auth, signed apt repository, performance audit (first pass done).

## Reading the project board

- **Todo**: not built yet.
- **In Progress**: built and tested on the simulators, waiting for a check on real hardware (the issues labelled `hardware-verification`; the steps are in [hardware-checklist.md](hardware-checklist.md)).
- **Done**: built, and verified where it needed hardware.
