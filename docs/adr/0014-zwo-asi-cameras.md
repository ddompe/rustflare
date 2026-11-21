# 0014. ZWO ASI cameras via a run-time loaded vendor library

- Status: Accepted
- Date: 2026-11-21

## Context

The owner has a ZWO ASI174MM Mini (USB `03c3:1749`, mono, Sony IMX174, global shutter). ASI cameras are not V4L2 devices; they need ZWO's `libASICamera2`. ZWO distributes the SDK from <https://www.zwoastro.com/software/product-sdk/> (ASI Camera SDK V1.41 at the time of writing, download only; we found no redistribution terms we could rely on). Debian and Raspberry Pi OS package it as `libasi` (1.27 on the Pi), built from the INDI third-party tree; it provides `libASICamera2.so` and a udev rule.

Unlike Player One (ADR 0006), we do not bundle this library: the licence for redistribution is unclear, and the package already exists in the distribution.

## Decision

- `crates/camera/src/asi.rs`, behind the Cargo feature `asi` (enabled in `scripts/build-deb.sh` and as `rustflare-server/asi`), registered in `rustflare_camera::drivers()`.
- The library is opened with `dlopen` through the `libloading` crate (names `libASICamera2.so`, `.so.1`; override with `RUSTFLARE_ASI_LIB`). Nothing is linked, no header or binary is committed, so the build works on any machine without the SDK, and CI is unaffected.
- Missing library: `enumerate` returns an empty list (logged once at info level) so users without ZWO cameras see no noise; `open` fails with `ZWO ASI SDK (libASICamera2) not found; install it with: sudo apt install libasi`.
- The `.deb` declares `Recommends: libasi`.
- Only the needed FFI is declared (about a dozen functions, three `repr(C)` structs written from `ASICamera2.h`, padded for newer SDK versions). The SDK calls sit behind a small private `Sdk` trait so control mapping, format handling and error paths are unit-tested with a fake.
- Capture uses video mode: `ASIStartVideoCapture` once, `ASIGetVideoData` per frame. A setting change stops the stream and the next `capture` re-arms it (so a frame never carries stale settings, important for flats). A timeout keeps the stream; other errors re-arm it.
- Controls use the standard ids (`exposure_us`, `gain`, `offset`, `high_bit_depth` = RAW16/RAW8), plus `usb_bandwidth`, set to its maximum on open. Ranges come from the camera (`ASIGetControlCaps`). Temperature is the SDK's `ASI_TEMPERATURE` divided by ten. Pixel size comes from the camera properties. Colour cameras are rejected for now (mono only).
- **udev:** `libasi` already installs `/usr/lib/udev/rules.d/99-asi.rules` (vendor `03c3`, mode 0666, `usbfs_memory_mb=200`), so RustFlare ships no extra rule. The owner's ZWO AM3 mount shares vendor id `03c3` but is a serial (cdc_acm) device; a USB node mode does not affect `/dev/ttyACM*`.

## Verified on hardware (Raspberry Pi 5, libasi 1.27, ASI174MM Mini)

Enumeration, open, controls and ranges, 8/16-bit full frames, temperature, pixel size. The camera enumerated at USB 2.0 speed, giving 8.7 fps (16-bit) and 17.3 fps (8-bit), limited by the link. ZWO's 164 fps needs USB 3 and was not measured. See `docs/hardware.md` for the numbers and sources (<https://www.zwoastro.com/product/mini-cameras/>: IMX174, 1936×1216, 5.86 µm, 12-bit, 3.5 e- read noise, 32 ke- full well, QE 77 %; the page's prose says "IMX249", the spec table says IMX174, and the pixel size and resolution match the IMX174).

## Consequences

- Users must install `libasi` (apt does by default through Recommends); the version in Debian (1.27) is older than ZWO's 1.41 and may not support the newest cameras.
- No ROI/binning, cooler or colour support yet; frame rate at full frame is bound by USB bandwidth, so a ROI would raise it.
- The SDK is serialised behind a mutex (it is not documented as thread-safe).
