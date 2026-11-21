# Roadmap & status

Work is tracked on the [GitHub project board](https://github.com/users/ddompe/projects/1) (one issue per item; closed = done). This page is the narrative summary.

**Where we are:** version 0.2.x, the first usable build. It has captured real H-alpha data with the owner's Player One Apollo-M MINI and ZWO AM3 on a Raspberry Pi 5. The next milestone is **v0.3.0**, the first release, once the open items under *Before v0.3.0* are done.

## Verified on the real system (first session, 2026-11-21)

Setup: Raspberry Pi 5 (Debian 13), Player One Apollo-M MINI (USB 3), ZWO AM3, H-alpha scope showing part of the disc (350 mm × 3 Barlow), FAT32 USB stick.

- **Camera:** enumeration, opening, controls, 1944×1472 16-bit frames, pixel size and temperature readout; the USB vendor id `a0a0` and the udev rule. **57 fps at 16-bit / 82 fps at 8-bit** in continuous mode (the one-shot mode gave a constant 8.4 fps).
- **Mount:** the AM3 connects over USB serial, solar tracking works, automatic calibration and guiding ran.
- **Tracking on real H-alpha frames** (offline, from the recordings): with the Optics radius the located limb centre is stable to ~2 px (σ) over a 100-frame burst, which is the seeing; frame-to-frame step ≈ 1.4–2 px mean. **The Sun's radius in the image is ≈ 1227 px, not the 1099 px the entered optics predict** (350 mm × a nominal 3× Barlow): the limb points fit the free circle to 3 px RMS but the optics circle to 21 px. So the effective focal length is ≈ 1170 mm (a real Barlow factor ≈ 3.35×). The tracker now measures the radius from the limb whenever the optics value does not fit the data, and the Optics panel suggests the corrected focal length / factor.
- **Bursts:** ten 100-frame bursts exactly 20 s apart, each file byte-exact (no frames lost), written to the FAT32 stick; auto-reconnect of camera and mount after a service restart; settings and camera exposure/gain persisted across restarts.
- **Packaging on the Pi:** install, upgrade, `rustflare-update` (including tampered-checksum abort), config and captures preserved.
- **CI:** tests, simulator smoke test and the arm64 package build pass on GitHub.

## Before v0.3.0 (first release)

1. **Open the recorded SER files in an independent tool** (SER Player, AutoStakkert, Siril) to confirm compatibility.
2. **Test the new drift-correcting guider on the real sky** (it replaced a guider that overshot at 0.88″/px; see [ADR 0008](adr/0008-solar-tracking-and-guiding.md)). Unit tests show it ignores seeing and corrects drift; the real mount has not seen it yet.
3. **FAT32 4 GB split and unplug behaviour** on the real stick.
4. **UI redesign** (a side agent is working on a simpler, responsive, iPhone-friendly UI), then deploy and check it on the phone.
5. **Decide on publishing the release**: the package bundles Player One's proprietary library ([issue #10](https://github.com/ddompe/rustflare/issues/10)).

## Planned

- **ZWO ASI camera support** (the owner has one to test): port Vidoxide's ASI driver to our camera trait.
- **EAF support and seeing-robust autofocus**: drive a ZWO EAF; compute a focus metric on an *average of several frames* (aligned on the limb) so turbulence does not corrupt the metric.
- **Device datasheets and inventory**: images and spec sheets (including the HCG point) for ZWO and Player One planetary/solar cameras and the AM3, shown in the Camera and Mount panels.
- **Keep mount tracking on across service restarts** (today connecting sends `:Td#`, switching tracking off).
- **iPhone web app**: responsive layout, Home Screen app (part of the UI redesign); HTTPS later enables a service worker and wake lock.
- **Field networking**: Pi hotspot / access-point mode for use away from Wi-Fi.
- **Guided flats** (cap-on, adjust, OK, capture, restore); darks; FITS output.
- **User guide** for observers (`docs/guide/`), with screenshots from real sessions.
- **Browser-level UI tests** (Playwright, Chromium and WebKit) in CI, with an opt-in profile for the real Pi.
- Signed apt repository; authentication and HTTPS; configurable guiding parameters; ROI/binning/cooler controls; colour cameras; more drivers.

## Known limitations

- Mono only (colour cameras deliver Bayer, treated as mono).
- The day-of-year solar radius model is good to ≈ 0.1 %: fine for tracking, not for astrometry.
- Limb fitting needs some limb in view; thresholds assume the sky is darker than the disc.
- Restarting the service switches the mount's tracking off (see Planned).
- No authentication; keep the Pi on a trusted network.
- The SER header's local-time field carries UTC.
- Released packages redistribute Player One's proprietary library (see [ADR 0006](adr/0006-playerone-via-vendor-sdk.md), [0010](adr/0010-debian-package-and-updates.md)).
