# Roadmap & status

Work is tracked in the [GitHub project](https://github.com/users/ddompe/projects) and the repository's issues; this page is the narrative summary.

## Verified

- **Unit tests** across all crates (trackers incl. H-alpha-like discs, calibration and guiding in closed loop, ZWO protocol on a mock port, SER/PNG, the asynchronous frame writer, storage logic, flats, solar geometry).
- **End to end on the simulators**, on a laptop and on a real Raspberry Pi 5 (Debian 13, aarch64): open camera, mount, solar tracking, tracking, automatic calibration, guiding, centre & guide, single PNG, sequences, scheduled bursts through the RAM buffer, PNG-folder output, flats (exposure converged and restored), error responses, preview.
- **Packaging on the Pi**: install, service start/enable, upgrade `0.1.0-1 → -2` preserving config and captures, `rustflare-update` (check, tampered-checksum abort, upgrade, up-to-date), the Player One library resolving via rpath next to libusb.
- Web UI checked visually on the simulators for the first version of the layout (live view, overlay, panels). Later panels (display controls, storage, optics, bursts) are type-checked and served but were not visually reviewed.

## Not verified yet: do these first on real hardware

1. **Player One camera**: enumerate/open, exposure range, real frame rate and the RAM buffer under a real high-fps load, udev rule (check `lsusb` ids).
2. **ZWO AM3**: model reply, `:TS#` solar rate, slew and guide directions, pulse behaviour, calibration result, guiding quality.
3. **Tracking on real H-alpha images**: Otsu separation, limb fit with a partial disc and the Optics radius, anchor on a sunspot or filament.
4. **USB drive**: selecting the FAT32 drive from the UI, writing, FAT32 rollover of SER files, behaviour when it is unplugged mid-capture.
5. **GitHub Actions** workflows (`ci.yml`, `release.yml`) and the first published release.
6. Sustained CPU and frame rate on the Pi with a real camera, preview and recording together.

## Planned

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
