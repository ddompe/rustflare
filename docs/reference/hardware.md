# Hardware

What RustFlare supports. Verification state is given per device; the open checks are in the [hardware checklist](../project/hardware-checklist.md).

## Player One cameras

Player One cameras are **not** V4L2/UVC devices. RustFlare drives them through Player One's userspace library
`libPlayerOneCamera`, using the `playerone-sdk` crate, which **bundles the library for Linux x86-64 and arm64** (and macOS).
No separate vendor SDK install is needed. The package includes the library and depends on `libusb-1.0-0` (installed automatically). When building from source use `--features playerone`; the build copies the library next to the binary, keep them together. See [ADR 0006](../adr/0006-playerone-via-vendor-sdk.md).

Controls exposed: exposure (µs), gain, offset, 8/16-bit output. Colour cameras deliver raw Bayer data, see [Colour cameras](capture-formats.md#colour-cameras).

!!! note "Status: verified on a real Apollo-M MINI (Raspberry Pi 5, USB 3)"
    Verified: USB vendor id `a0a0` (the shipped udev rule matches and grants access), enumeration, opening, controls (exposure 10 µs–2000 s, gain 0–550, offset 0–1000, 8/16-bit), 1944×1472 frames, pixel size 4.5 µm (used by the Optics panel), temperature readout.
    **SD card write speed** (Pi 5): ≈ 65 MB/s sustained (572 MB burst flushed in ≈ 8.7 s), i.e. ≈ 11 fps of full-frame 16-bit video; the RAM buffer is what makes faster bursts possible.
    **Frame rate:** the first version used the SDK's one-shot capture, which re-arms the camera for every frame and measured a constant **8.4 fps (9.8 fps at 8-bit) whatever the exposure**. The driver now uses the SDK's continuous mode (arm once, fetch frames). **Measured after the change: 57 fps at 16-bit and 82 fps at 8-bit (1944×1472), constant from 10 ms down to 0.1 ms exposure**, about 7–8× faster, so the camera or USB link is now the limit.

## ZWO ASI cameras

ZWO ASI cameras are driven through ZWO's userspace library `libASICamera2`, which RustFlare **loads at run time** (`dlopen`); it is never linked or bundled. On Raspberry Pi OS / Debian install it with `sudo apt install libasi` (the `.deb` lists it as *Recommends*, so apt installs it by default). Without it the ASI driver lists no cameras and opening one reports `install it with: sudo apt install libasi`. Build with `--features asi`; see [ADR 0014](../adr/0014-zwo-asi-cameras.md).

Controls exposed: exposure (µs), gain, offset, USB bandwidth (%), 8/16-bit output, and a region of interest (Camera tab > Frame size, centred 75, 50 or 25 %, set with `ASISetROIFormat`; not yet tried on a real camera). For a cooled camera also the **cooler** (see below). Colour cameras are reported with their Bayer pattern. The `libasi` package ships its own udev rule (`/usr/lib/udev/rules.d/99-asi.rules`: vendor id `03c3`, mode 0666, and `usbfs_memory_mb=200`), so RustFlare adds none. The same vendor id covers the ZWO AM3/AM5 mount (a serial device), which the rule leaves usable.

**ASI174MM Mini** (USB id `03c3:1749`), from [ZWO's Mini Cameras page](https://www.zwoastro.com/product/mini-cameras/): Sony IMX174 (global shutter), 1/1.2" (11.3 × 7.1 mm), 1936 × 1216, 5.86 µm pixels, 12-bit ADC, read noise 3.5 e-, full well 32 ke-, peak QE 77 %, USB 3.0, up to 164 fps (ZWO's figure).

!!! note "Status: verified on a real ASI174MM Mini (Raspberry Pi 5, libasi 1.27)"
    Verified: enumeration, opening, controls (exposure 32 µs to 2000 s, gain 0-400, offset 0-240, USB bandwidth 40-100 %, 8/16-bit), 1936 × 1216 frames, pixel size 5.86 µm, temperature readout (about 38-41 °C idle), RAW16 scaled to the full 16-bit range (mean level matches the 8-bit stream).
    **Frame rate:** the camera enumerated at **USB 2.0 speed (480 Mbit/s)** on the Pi's USB 2 port, so it measured **8.7 fps at 16-bit and 17.3 fps at 8-bit (full frame)**, independent of exposure from 1 to 10 ms: the USB 2 link (about 40 MB/s) is the limit, not the driver. ZWO's 164 fps needs a USB 3 port and cable (blue Pi port), and was **not measured**.


### ZWO cooled cameras (ASI533MC Pro and relatives): cooler, fan, anti-dew heater

The driver offers a cooler only when the camera says `IsCoolerCam` **and** the SDK lists a writable `ASI_COOLER_ON` control (`ASIGetControlCaps`). The fan (`ASI_FAN_ON`) and the anti-dew heater (`ASI_ANTI_DEW_HEATER`) appear only where the SDK lists them, the target range is the one the SDK reports for `ASI_TARGET_TEMP`. Control numbers (counted from the `ASI_CONTROL_TYPE` enum of `ASICamera2.h`): `ASI_TEMPERATURE` 8 (ten times the temperature), `ASI_COOLER_POWER_PERC` 15 (read only), `ASI_TARGET_TEMP` 16 (whole degrees, not times ten), `ASI_COOLER_ON` 17, `ASI_FAN_ON` 19, `ASI_ANTI_DEW_HEATER` 21. Changing them does not stop a running stream. The Camera tab and `PUT /api/camera/cooler` ([API](api.md)) drive them; switching off a cold sensor ramps the target first (see the [camera guide](../guide/camera.md)). The Player One driver does not expose its cooler yet (the SDK has the calls; the trait takes them).

!!! note "Status: tested against a fake SDK and the simulator only"
    Not yet tried on a real cooled ZWO camera. To check on one: the Cooler section appears; setting a target of -10 and switching on lowers the sensor temperature and raises the power; the fan and heater switches click through; switching off while cold warms by 1 degree per 15 s and then the cooler turns off.

**ASI533MC Pro** (IMX533, colour), from [ZWO's ASI533 Pro page](https://www.zwoastro.com/product/asi533-pro-series/), [its product page](https://www.zwoastro.com/product/asi533mc-pro/) and the [ASI533 manual](https://astronomy-imaging-camera.com/manuals/ASI533_Manual_EN.pdf): Sony IMX533, 1" (11.31 × 11.31 mm), 3008 × 3008, 3.76 µm pixels, 14-bit ADC, full well 50 ke-, read noise 1.0 to 3.8 e- depending on gain (1.53 e- at gain 100), peak QE about 80 % (colour), 20 fps at full resolution on USB 3.0, two-stage TEC with Delta T 35 °C below ambient, DDR3 256 MB. Gain 100 (offset 70) is ZWO's default "Unity Gain" setting for it, and gain 100 is where ZWO's cameras of this family switch on high conversion gain (stated in ZWO's ASI2600 text; the ASI533 pages do not say "HCG", so the catalog marks it and says so).

The catalog (`crates/server/data/catalog.json`) carries `hcg_gain` for the ZWO cameras whose switch point ZWO's own pages state: ASI294 (120), ASI662 (200), ASI2600 (100), ASI664 (180) and the ASI533 (100, see above). Not stated on ZWO's ASI585, ASI678, ASI715, ASI676 or ASI183 pages as fetched, so they have none yet (retailers quote 252 for the ASI585 and 182 for the ASI678; add them when ZWO's manual is checked).


## ZWO AM3 / AM5 mounts

Connected over USB as a serial device (it appears as `/dev/ttyACM0`, and as a stable name under `/dev/serial/by-id/`; **Find my mount** on the Aim tab asks each port `:GVP#` at 9600 baud and names the model that answers); RustFlare speaks the ZWO serial protocol directly (no ASCOM/INDI).
The AM3 can run equatorial or alt-az; RustFlare calls the axes *primary/secondary* (RA/Dec in equatorial mode). See [Calibration](tracking-and-guiding.md#calibration) for what that means for guiding. Supported: solar tracking rate (`:TS#`), tracking on/off, manual slewing at the ten speeds the mount offers
(0.25× … 1440× sidereal; the Aim tab builds its speed list from the driver), pulse guiding. Guide rate is clamped to the mount's 0.1–0.9× range; the mount has one rate for both axes.
The mount can also be connected over **Bluetooth** (Aim tab: Connection > Bluetooth > Scan for mounts; [ADR 0015](../adr/0015-ble-mount-transport.md)). Press the mount's pairing button and close the ZWO phone app first; the Pi needs `bluez` (a Recommends of the package). The Bluetooth link is verified on a real AM3 (see Status below). Protocol notes for developers: [ZWO mount Bluetooth protocol](../project/zwo-ble-protocol.md).

!!! note "Status"
    Verified on a real ZWO AM3 over USB and Bluetooth: connect, tracking, nudge, Send home and Go to Sun. Other models (AM3N, AM5, AM5N) probably behave the same but have not been tried. Guiding on a real sky and the meridian flip assist are not yet verified.


## iOptron mounts (CEM70 series, HAE69 and relatives)

Driver name `ioptron` (Aim tab > Mount). Connect the mount's USB port to the Pi; it appears as a serial port (`/dev/ttyACM0` or `/dev/ttyUSB0`). **Find my mount** asks each port `:MountInfo#` at 115200 baud and names the model (`iOptron HAE69C`). RustFlare speaks the **iOptron RS-232 command language 2014 version 3.10** at 115200 baud, 8N1 (the settings are applied for you). The document lists the CEM120, CEM70, GEM45, CEM40, GEM28 and CEM26 series; the HAE69 is driven with the same language (that is how AsiAir treats it). The first thing sent is `:MountInfo#`, which names the model; a port that does not answer says so. A real HAE69C answers `0066`, which the driver names *HAE69C* and gives a top slew speed of about 1077 times sidereal (ioptron.rs says this is the vendor's 4.5 degrees a second, not from the protocol document). The mount often takes 300 to 600 ms to answer (`:ST1#` for tracking and `:qR#` for stop were seen), so the driver waits up to 2 seconds for a reply and drops stray input before each command.

What works through the protocol: Go to (`:MS1#`, falling back to the counterweight-up position `:MS2#`), tracking on and off, sidereal, lunar and **solar** tracking rates, manual slews (rates from 1x to 512x sidereal and the mount's maximum, which depends on the model and is not offered for a model code the driver does not know), **Search home** (`:MSH#`, finds the mechanical zero with the homing sensors and makes it the new zero), timed guide pulses (`:ZS`, `:ZQ`, `:ZE`, `:ZC`), sending the site and time, reading the pointing and the state (tracking, slewing, parked, at the zero position), and Send home (`:MH#`). A meridian flip is a new Go to to the same place: the mount's own *meridian treatment* setting (stop or flip, and how many degrees past the meridian) decides what it does near the meridian.

!!! note "Status: connected to a real HAE69C (2026-11-26 and 27); most of it is still tested against a mock only"
    The driver was written from the protocol document and tested against a mock (`cargo test -p rustflare-mount`). A real HAE69C has since been connected over USB: it answered `:MountInfo#` with `0066`, and slews, Send home, Search home, tracking, and reading and writing the site and time have been used on it (the roadmap lists them as verified; tracking and stop needed the 2 s reply timeout). **Not yet tried on the real mount** (per the checklist): guide pulses and guiding, Go to Sun after a time sync, and a meridian pass. Check the slew pad directions on yours (the signs of RA and Dec in the document are ambiguous; calibration absorbs them for guiding but the pad does not). Report surprises (Aim tab, or `GET /api/debug/mount`, which logs every command and reply). The checklist rows are in the [hardware checklist](../project/hardware-checklist.md).

### The mount's own site and time (read back)

`Mount::read_site_time` reads what the mount holds, without changing it (Aim tab: *Mount time and place*; `status.mount.site_time`).

- **iOptron** (protocol v3.10): `:GLS#` answers a sign and 22 digits; the first 8 are the longitude in 0.01 arc-seconds (the sign applies to it, east positive) and the next 8 are the latitude **plus 90 degrees** in 0.01 arc-seconds. `:GUT#` answers `sMMM` (minutes from UTC, no daylight saving in it), one daylight-saving flag digit and 13 digits of milliseconds since J2000.0 (Unix milliseconds of J2000.0 are 946728000000), 18 characters before the `#`. A real HAE69C answered `:GLS#` in this layout (its site matched the owner's real one, not reproduced here) and `:GUT#` `-36000536551185000` (UTC-6, no daylight saving, 2017-01-01 13:59:45 UTC: the clock had never been set). The HAE69C has no clock battery: it restarts from 2017-01-01 14:00 UTC each time it is switched on, which is why the Pi can send its site and time by itself after connecting. Whether the clock then keeps ticking after a write is not yet known. Every command can take 300 to 600 ms on this mount, so the engine reads these every 30 s, not with the 2 s poll.
- **ZWO AM3/AM5**: the clock only, from `:GAST#` (`MM/DD/YY&HH:MM:SS&+06:00#RA&Dec#flags#`). The date and time are the mount's **local** time and the offset has the opposite sign to the usual one (`+06:00` is UTC-6). No command that reads the mount's site back is documented in this repository, so latitude and longitude are reported as unknown rather than guessed.
- The clock and place are read on connecting and every 30 s. Writing is `set_site_time` (ZWO `:SMGE#` and `:SMTI#`; iOptron `:SG`, `:SDS`, `:SUT`, `:SLO`, `:SLA`, `:SHE`); the Aim tab button, `POST /api/mount/sync-site-time` and the automatic send after connecting (`PUT /api/mount/auto-site-time`, on by default, only with a synchronised Pi clock, a saved site and a mount that is not moving) send the Pi's saved site and clock and read them back. They move nothing.
- Tolerances (`crates/server/src/sitetime.rs`): the clock differs above 5 s, the place above 0.05 degrees of arc (longitude wraps at 180).

## Simulators

`simulator` camera and mount are always available. The simulated Sun drifts, responds to mount motion, and has a *Sim scene* control (0 = Sun, 1 = vignetted flat field for testing flats, 2 = Moon with a phase (*Sim Moon phase*, 0 new, 0.5 full), 3 = a 25 px planet with two small moons and *Sim seeing jitter*). Scenes 2 and 3 start at the frame centre and move with the mount like the Sun; on the colour simulator the Moon is nearly neutral and the planet orange.

To add a driver for another camera or mount, see the [development guide](../project/development.md#adding-a-camera-or-mount).
