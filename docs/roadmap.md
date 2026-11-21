# Roadmap & status

Work is tracked on the [GitHub project board](https://github.com/users/ddompe/projects/1) (one issue per item, closed = done); this page is the narrative summary. `scripts/create-project-board.py` created the board from this list.

## Verified

- **Unit tests** across all crates (trackers incl. H-alpha-like discs, calibration and guiding in closed loop, ZWO protocol on a mock port, SER/PNG, the asynchronous frame writer, storage logic, flats, solar geometry).
- **End to end on the simulators**, on a laptop and on a real Raspberry Pi 5 (Debian 13, aarch64): open camera, mount, solar tracking, tracking, automatic calibration, guiding, centre & guide, single PNG, sequences, scheduled bursts through the RAM buffer, PNG-folder output, flats (exposure converged and restored), error responses, preview.
- **Packaging on the Pi**: install, service start/enable, upgrade `0.1.0-1 → -2` preserving config and captures, `rustflare-update` (check, tampered-checksum abort, upgrade, up-to-date), the Player One library resolving via rpath next to libusb.
- Web UI checked visually on the simulators for the first version of the layout (live view, overlay, panels). Later panels (display controls, storage, optics, bursts) are type-checked and served but were not visually reviewed.

## Not verified yet: do these first on real hardware

1. **Player One camera** (partly verified): enumeration, opening, controls and the udev rule are verified on an Apollo-M MINI. Frame rate with continuous mode **measured: 57 fps 16-bit / 82 fps 8-bit** (was a constant 8.4 with one-shot capture). A 100-frame, 572 MB burst was captured through the RAM buffer with no frames lost and flushed to the SD card at ≈ 65 MB/s; camera auto-reconnect after a service restart works. **Still to check:** longer runs and scheduled bursts under real load, and the USB stick.
2. **ZWO AM3**: model reply, `:TS#` solar rate, slew and guide directions, pulse behaviour, calibration result, guiding quality.
3. **Tracking on real H-alpha images**: Otsu separation, limb fit with a partial disc and the Optics radius, anchor on a sunspot or filament.
4. **USB drive**: selecting the FAT32 drive from the UI, writing, FAT32 rollover of SER files, behaviour when it is unplugged mid-capture.
5. **GitHub Actions** workflows (`ci.yml`, `release.yml`) and the first published release.
6. Sustained CPU and frame rate on the Pi with a real camera, preview and recording together.

## Planned

- *(done in 0.2.1, unverified)* Streaming capture via the SDK's continuous mode, auto-reconnect of camera and mount at startup, red adjustable bullseye, serial-port suggestions for the mount.

- **Guided flats.** Flats work today but start instantly. Planned workflow: *Flats* opens a step-by-step dialog: (1) "Put the flat cap/diffuser on and make sure the field is uniform", showing the live view and histogram; (2) adjust gain/exposure (with a suggested exposure and a clipping/underexposure indicator), tracking and guiding paused; (3) **OK** captures N flats; (4) results are summarised (level, frames, file, master) and the previous settings restored, with a reminder to remove the cap. Also to decide: matching darks/dark flats, naming that ties flats to the lights' settings (gain/offset/format/exposure), and flats per filter/position. Deferred until the base setup is stable on real hardware.

- **User guide.** A task-oriented guide for observers (not just reference docs): *setting up at the scope; getting the Sun in view and centred; an H-alpha session from first light to files; planning bursts and storage; flats; reading the screen (overlays, histogram, status); troubleshooting and FAQ*, with screenshots from real sessions. It should live in `docs/guide/` and be linked from the README and the UI.
- **Browser-level UI tests** (Playwright) in CI against the simulator, with screenshots as artifacts; an opt-in profile to run the same checks against a real Pi (never moving the mount without an explicit flag).
- Signed apt repository (GitHub Pages) so `apt upgrade` works without `rustflare-update`.
- Streaming capture with the SDK's `stream` mode for higher frame rates.
- Optional drop-frames policy for full-buffer situations (lucky imaging); FITS output; dark frames.
- ROI, binning and cooler/temperature controls; colour (Bayer) cameras.
- Authentication (token) and HTTPS; configurable guiding parameters; refreshable anchor template.
- More drivers: ZWO ASI, V4L2 cameras; ASCOM/INDI bridges; AM3/AM5 position readout and GoTo.

## Known limitations

- Frame rate is limited by per-frame exposure start/stop (one SDK call per frame) until streaming mode is used.
- Mono only (colour cameras deliver Bayer, treated as mono).
- Day-of-year solar radius model is good to ≈ 0.1 %: fine for tracking, not for astrometry.
- Limb fitting needs some limb in view; thresholds assume the sky is darker than the disc.
- No authentication; keep the Pi on a trusted network.
- The SER header's local-time field carries UTC.
- Released packages redistribute Player One's proprietary library (see [ADR 0006](adr/0006-playerone-via-vendor-sdk.md), [0010](adr/0010-debian-package-and-updates.md)).
