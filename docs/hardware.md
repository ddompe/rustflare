# Hardware

## Player One cameras

Player One cameras are **not** V4L2/UVC devices. Rustflare drives them through Player One's userspace library
`libPlayerOneCamera`, using the `playerone-sdk` crate, which **bundles the library for Linux x86-64 and arm64** (and macOS).
No separate vendor SDK install is needed. The package includes the library and depends on `libusb-1.0-0` (installed automatically). When building from source use `--features playerone`; the build copies the library next to the binary, keep them together. See [ADR 0006](adr/0006-playerone-via-vendor-sdk.md).

Controls exposed: exposure (µs), gain, offset, 8/16-bit output. Colour cameras deliver raw Bayer data, treated as mono.

!!! note "Status"
    Verified on the Pi: the library loads and enumeration runs (no camera attached yet, so nothing is listed). **Not yet run with a real camera.** First things to check: enumeration, opening, exposure range, frame rate, and the udev rule (the vendor id `a0a0` is unverified; compare with `lsusb`).

## ZWO AM3 / AM5 mounts

Connected over USB as a serial device; Rustflare speaks the ZWO serial protocol directly (no ASCOM/INDI).
The AM3 can run equatorial or alt-az; Rustflare calls the axes *primary/secondary* (RA/Dec in equatorial mode). See [Calibration](tracking-and-guiding.md#calibration) for what that means for guiding. Supported: solar tracking rate (`:TS#`), tracking on/off, manual slewing at the ten speeds the mount offers
(0.25× … 1440× sidereal), pulse guiding. Guide rate is clamped to the mount's 0.1–0.9× range; the mount has one rate for both axes.
See [ADR 0007](adr/0007-zwo-mount-serial-driver.md) for the fixes made relative to Vidoxide's driver.

!!! note "Status"
    Unit-tested against a mock port; upstream's version was tested on AM3 and AM5. Verify on first use: model string, solar rate, guide direction (calibration discovers it).

## Simulators

`simulator` camera and mount are always available. The simulated Sun drifts, responds to mount motion, and has a *Sim scene* control (0 = Sun, 1 = vignetted flat field) for testing flats.

## Adding a camera or mount

1. Implement `rustflare_core::{CameraDriver, Camera}` (or `Mount`) in a new file under `crates/camera/src/` (or `crates/mount/src/`).
2. Register it in `rustflare_camera::drivers()` / `rustflare_mount::connect()` — behind a Cargo feature if it needs native libraries.
3. Report settings as `ControlDesc`s; use the standard ids (`exposure_us`, `gain`, `offset`, `high_bit_depth`) where they apply so flats and the UI work unchanged.
4. Add a simulator-style or mock-I/O test. The UI needs no change: it renders whatever controls the driver returns.
