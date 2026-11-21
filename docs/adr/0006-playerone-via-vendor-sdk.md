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

## Consequences

- On **macOS** the library's install name is a bare `libPlayerOneCamera.3.dylib` (not `@rpath/…`), so a `playerone` build compiles but will not launch there without `install_name_tool`; macOS is for development with the simulator, the Pi (Linux) is the target. The Linux rpath/SONAME path has not been run (no Linux machine was available when written).
- The default build has no native dependencies and runs on any machine with the simulator.
- Per-frame start/stop-exposure calls limit the achievable frame rate versus the SDK's streaming mode; switching to `Camera::stream` is a planned optimisation.
- The vendor library is closed source and is redistributed inside the crate; check Player One's terms before redistributing binaries.
- Colour sensors: frames arrive as raw Bayer and are treated as mono; proper colour support is future work.
