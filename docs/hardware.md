# Hardware

## Player One cameras

Player One cameras are **not** V4L2/UVC devices. RustFlare drives them through Player One's userspace library
`libPlayerOneCamera`, using the `playerone-sdk` crate, which **bundles the library for Linux x86-64 and arm64** (and macOS).
No separate vendor SDK install is needed. The package includes the library and depends on `libusb-1.0-0` (installed automatically). When building from source use `--features playerone`; the build copies the library next to the binary, keep them together. See [ADR 0006](adr/0006-playerone-via-vendor-sdk.md).

Controls exposed: exposure (µs), gain, offset, 8/16-bit output. Colour cameras deliver raw Bayer data, treated as mono.

!!! note "Status: verified on a real Apollo-M MINI (Raspberry Pi 5, USB 3)"
    Verified: USB vendor id `a0a0` (the shipped udev rule matches and grants access), enumeration, opening, controls (exposure 10 µs–2000 s, gain 0–550, offset 0–1000, 8/16-bit), 1944×1472 frames, pixel size 4.5 µm (used by the Optics panel), temperature readout.
    **SD card write speed** (Pi 5): ≈ 65 MB/s sustained (572 MB burst flushed in ≈ 8.7 s), i.e. ≈ 11 fps of full-frame 16-bit video; the RAM buffer is what makes faster bursts possible.
    **Frame rate:** the first version used the SDK's one-shot capture, which re-arms the camera for every frame and measured a constant **8.4 fps (9.8 fps at 8-bit) whatever the exposure**. The driver now uses the SDK's continuous mode (arm once, fetch frames). **Measured after the change: 57 fps at 16-bit and 82 fps at 8-bit (1944×1472), constant from 10 ms down to 0.1 ms exposure**, about 7–8× faster, so the camera or USB link is now the limit.

## ZWO ASI cameras

ZWO ASI cameras are driven through ZWO's userspace library `libASICamera2`, which RustFlare **loads at run time** (`dlopen`); it is never linked or bundled. On Raspberry Pi OS / Debian install it with `sudo apt install libasi` (the `.deb` lists it as *Recommends*, so apt installs it by default). Without it the ASI driver lists no cameras and opening one reports `install it with: sudo apt install libasi`. Build with `--features asi`; see [ADR 0014](adr/0014-zwo-asi-cameras.md).

Controls exposed: exposure (µs), gain, offset, USB bandwidth (%), 8/16-bit output. Mono cameras only for now. The `libasi` package ships its own udev rule (`/usr/lib/udev/rules.d/99-asi.rules`: vendor id `03c3`, mode 0666, and `usbfs_memory_mb=200`), so RustFlare adds none. The same vendor id covers the ZWO AM3/AM5 mount (a serial device), which the rule leaves usable.

**ASI174MM Mini** (USB id `03c3:1749`), from [ZWO's Mini Cameras page](https://www.zwoastro.com/product/mini-cameras/): Sony IMX174 (global shutter), 1/1.2" (11.3 × 7.1 mm), 1936 × 1216, 5.86 µm pixels, 12-bit ADC, read noise 3.5 e-, full well 32 ke-, peak QE 77 %, USB 3.0, up to 164 fps (ZWO's figure).

!!! note "Status: verified on a real ASI174MM Mini (Raspberry Pi 5, libasi 1.27)"
    Verified: enumeration, opening, controls (exposure 32 µs to 2000 s, gain 0-400, offset 0-240, USB bandwidth 40-100 %, 8/16-bit), 1936 × 1216 frames, pixel size 5.86 µm, temperature readout (about 38-41 °C idle), RAW16 scaled to the full 16-bit range (mean level matches the 8-bit stream).
    **Frame rate:** the camera enumerated at **USB 2.0 speed (480 Mbit/s)** on the Pi's USB 2 port, so it measured **8.7 fps at 16-bit and 17.3 fps at 8-bit (full frame)**, independent of exposure from 1 to 10 ms: the USB 2 link (about 40 MB/s) is the limit, not the driver. ZWO's 164 fps needs a USB 3 port and cable (blue Pi port), and was **not measured**.


Connected over USB as a serial device (it appears as `/dev/ttyACM0`, and as a stable name under `/dev/serial/by-id/`, which the Mount panel suggests); RustFlare speaks the ZWO serial protocol directly (no ASCOM/INDI).
The AM3 can run equatorial or alt-az; RustFlare calls the axes *primary/secondary* (RA/Dec in equatorial mode). See [Calibration](tracking-and-guiding.md#calibration) for what that means for guiding. Supported: solar tracking rate (`:TS#`), tracking on/off, manual slewing at the ten speeds the mount offers
(0.25× … 1440× sidereal), pulse guiding. Guide rate is clamped to the mount's 0.1–0.9× range; the mount has one rate for both axes.
See [ADR 0007](adr/0007-zwo-mount-serial-driver.md) for the fixes made relative to Vidoxide's driver. The mount can also be connected over **Bluetooth** (Aim tab: Connection > Bluetooth > Scan for mounts; [ADR 0015](adr/0015-ble-mount-transport.md)). Press the mount's pairing button and close the ZWO phone app first; the Pi needs `bluez` (a Recommends of the package). *Implemented but not yet verified on a real mount.* What we know about the link is in [ZWO mount Bluetooth protocol](zwo-ble-protocol.md).

!!! note "Status"
    Unit-tested against a mock port; upstream's version was tested on AM3 and AM5. Verify on first use: model string, solar rate, guide direction (calibration discovers it).

## Simulators

`simulator` camera and mount are always available. The simulated Sun drifts, responds to mount motion, and has a *Sim scene* control (0 = Sun, 1 = vignetted flat field) for testing flats.

## Adding a camera or mount

1. Implement `rustflare_core::{CameraDriver, Camera}` (or `Mount`) in a new file under `crates/camera/src/` (or `crates/mount/src/`).
2. Register it in `rustflare_camera::drivers()` / `rustflare_mount::connect()` — behind a Cargo feature if it needs native libraries.
3. Report settings as `ControlDesc`s; use the standard ids (`exposure_us`, `gain`, `offset`, `high_bit_depth`) where they apply so flats and the UI work unchanged.
4. Add a simulator-style or mock-I/O test. The UI needs no change: it renders whatever controls the driver returns.
