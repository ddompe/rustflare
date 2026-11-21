# 0006. Player One cameras via the vendor SDK, not V4L2

- Status: Accepted — **not yet tested on real hardware**
- Date: 2026-11-21

## Context

The expectation was that Player One cameras "just work" on Linux. Checked:

- They are **not** UVC/V4L2 devices. They are controlled through Player One's own userspace library, `libPlayerOneCamera` (INDI and INDIGO both ship dedicated drivers around it).
- *No separate SDK installation is needed on the Pi*, though: the `playerone-sdk-sys` crate (v0.1.1, SDK 3.7.0) **bundles** `libPlayerOneCamera.so` for linux x86-64 and **arm64**, and a macOS dylib, and links it at build time.
- **But the bundled library is not self-sufficient** (found by building and running it): its SONAME is `libPlayerOneCamera.so.3` while the crate ships the file as `libPlayerOneCamera.so`, the crate sets no rpath, so a binary built with the feature fails to start (`Library not loaded`). It also depends on the system `libusb-1.0.so.0` (`apt install libusb-1.0-0`).
- Vidoxide has no Player One driver (it has ASI, V4L2, IIDC, FlyCapture2, Spinnaker).

Not verified: whether the camera needs a udev rule for non-root access (very likely, as for most USB astronomy cameras; the rule shipped in `deploy/` uses the Player One vendor id and should be confirmed with `lsusb`).

## Decision

Implement `PlayerOneDriver` with the `playerone-sdk` crate (idiomatic wrapper over the sys crate), behind the
`playerone` Cargo feature. Frames are captured with the SDK's blocking `capture`, in RAW16 or RAW8
(`high_bit_depth` control). Build with `--features playerone` on the Pi (bindgen needs `libclang`).

The server's `build.rs` (feature `playerone` only) adds an executable-relative rpath (`$ORIGIN`, `$ORIGIN/lib`) and copies the library from the cargo registry next to the binary under its SONAME. A deployed install is therefore: binary + `libPlayerOneCamera.so.3` in the same folder, plus `libusb-1.0-0` from apt.

## Capture mode (found on real hardware)

The `playerone-sdk` wrapper's `capture()` starts every frame as a single shot (`POAStartExposure(…, true)`), re-arming the camera each time. On an Apollo-M MINI this gave a constant 8.4 fps (9.8 fps at 8-bit) independent of exposure (10 ms and 1 ms identical), i.e. a fixed per-frame cost, not exposure time or USB bandwidth. The driver therefore uses the SDK's continuous mode: `start_exposure()` once (`POA_FALSE`), then `get_image_data()` per frame; any setting change stops the stream and the next capture restarts it. Measurement of the new rate is pending.

## Consequences

- On **macOS** the library's install name is a bare `libPlayerOneCamera.3.dylib` (not `@rpath/…`), so a `playerone` build compiles but will not launch there without `install_name_tool`; macOS is for development with the simulator, the Pi (Linux) is the target. The Linux rpath/SONAME path has not been run (no Linux machine was available when written).
- The default build has no native dependencies and runs on any machine with the simulator.
- Per-frame start/stop-exposure calls limit the achievable frame rate versus the SDK's streaming mode; switching to `Camera::stream` is a planned optimisation.
- The vendor library is closed source. **Redistribution decision (2026-11-21):** the SDK's own `license.txt` (v3.10.1) says "You can use our company's products and this SDK to develop any products without any restrictions" and requires its copyright and permission notice to be included in all copies. We therefore ship `libPlayerOneCamera.so.3` in the Debian package and include the licence text as `/usr/share/doc/rustflare/PlayerOne-SDK-LICENSE.txt` (source: `packaging/licenses/`). This is the project owner's reading of the licence, not legal advice; if Player One objects, the library can be made a separate download.
- Colour sensors: frames arrive as raw Bayer and are treated as mono; proper colour support is future work.
