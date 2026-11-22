<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/img/logo-mark-dark.svg">
    <img src="docs/img/logo-mark-light.svg" alt="RustFlare" width="128">
  </picture>
</p>

<h1 align="center">RustFlare</h1>

Headless, web-controlled **solar capture** in Rust. Run it on a Raspberry Pi next to the telescope, open a web page from your phone or laptop (an iPhone Home Screen app works well), and take the Sun from "where is it?" to a saved recording without a keyboard.

## Features

**Mount** (ZWO AM3 family, over USB or Bluetooth)
- **Go to Sun**: slews to the Sun's current position from your site and the Pi's clock, then switches on solar tracking. **Send home**, a slew pad with remembered speed, and the state the mount itself reports (it follows changes made with the mount's own buttons).
- USB re-enumeration and Bluetooth link loss are detected; the mount is found again automatically.

**Cameras** (Player One, and ZWO ASI through the vendor library; the ASI driver is written but not yet run on a real camera)
- **Frame size (ROI)**: read out the centre 75/50/25 % of the sensor for a much higher frame rate (57 fps full frame, 153 fps at 50 % on the Apollo-M MINI over USB 3).
- Exposure, gain, offset and 8/16-bit with a logarithmic exposure slider, histogram with clipping warning, sensor temperature, device datasheet card.
- **Darks** (cap on, same exposure and gain) and **flats**: guided workflow (cap on, check level, capture), automatic exposure to about 50 %, master flat applied to the live view (recordings stay raw) and remembered across restarts.

**Tracking and guiding**
- Follows the **full disc**, a **partial disc** (fitted from the visible limb, with the expected radius from your optics and a measured fallback) or a **surface feature** such as a sunspot.
- Automatic calibration of both mount axes, then a guider that corrects **slow drift only**: it averages away the seeing, uses a dead band and gentle pulses. All parameters are adjustable.

**Live view**
- Draggable, semi-transparent **overlays**: camera/screen/disk fps, exposure, gain, temperature, mount state, flat applied, recording, RAM buffer, guiding error graph, histogram.
- Centring aids (bullseye, Sun outline), zoom, brightness/contrast/gamma, auto-stretch, H-alpha tint.
- **Prominence view**: a software coronagraph that blacks out the disc so you can judge the exposure for prominences. **Prominence pair** capture saves a disc frame, a longer exposure and their composite.

**Recording**
- **Single frames, sequences and scheduled bursts** ("100 frames every 30 s") as **SER** videos, **PNG** or **FITS** files, through a **RAM buffer** so a slow disk never limits a fast camera; choose to wait for the disk or **drop frames** when the buffer is full.
- **USB drive** selection with free-space and FAT32 safeguards (4 GB files continue in numbered parts) and a **safe eject** button.
- A full-screen **file manager**: browse by day, check recordings (frame count, measured fps, brightness; flags *mostly dark* and *incomplete*), download, delete in bulk.

**Running it**
- Installs and upgrades as a Debian package (`rustflare-update`, or a signed apt repository); settings (site, flats, guiding, storage, UI layout) survive reboots; auto-reconnect of camera and mount.
- **User guide** with screenshots: [docs/guide/](docs/index.md), published at https://ddompe.github.io/rustflare/manual/. Start with [Getting started](docs/guide/getting-started.md); problems are covered in [Troubleshooting](docs/guide/troubleshooting.md).
- **Field Wi-Fi hotspot** (`rustflare-hotspot`) for use without a network, with an automatic way back to the previous Wi-Fi ([docs/guide/hotspot.md](docs/guide/hotspot.md)).
- Opt-in **session log** (what the tracker and guider saw, plus small frames) and a replay test harness for building regression fixtures from real sessions.
- Optional **password** and **HTTPS** with a private certificate authority you trust once on your phone (`rustflare-tls`).
- Built-in simulators for the camera and mount, so everything can be tried without hardware; Playwright browser tests run in CI.

Based on ideas and code from [Vidoxide](https://github.com/GreatAttractor/vidoxide) (MIT), see [NOTICE.md](NOTICE.md).

## Install on a Raspberry Pi

From a release package:

```sh
# https://github.com/ddompe/rustflare/releases/latest
sudo apt install ./rustflare_*_arm64.deb
sudo rustflare-update          # later: install the newest release
```

Or from the signed apt repository (see [docs/guide/updating.md](docs/guide/updating.md)):

```sh
curl -fsSL https://ddompe.github.io/rustflare/rustflare.gpg | sudo tee /usr/share/keyrings/rustflare.gpg >/dev/null
echo "deb [signed-by=/usr/share/keyrings/rustflare.gpg arch=arm64] https://ddompe.github.io/rustflare stable main" | sudo tee /etc/apt/sources.list.d/rustflare.list
sudo apt update && sudo apt install rustflare
```

Then open `http://<pi-name>.local:8080/`. For HTTPS and a password see [docs/guide/phone-setup.md](docs/guide/phone-setup.md). Details in [docs/guide/getting-started.md](docs/guide/getting-started.md); how to use it in the [tour of the screen](docs/guide/screen.md).

## Status

Developed against a Raspberry Pi 5, a Player One Apollo-M MINI and a ZWO AM3.

| Area | State |
|---|---|
| ZWO AM3 over USB and Bluetooth: connect, tracking, nudge, send home, Go to Sun | **Verified on the real mount** |
| Capture path: camera and USB stick on separate USB 3 controllers (~57 fps at 1944x1472, 3.4 GB sequence) | **Verified on the Pi** |
| Storage, 4 GB FAT32 split, camera settings restore, package install and upgrade | **Verified on the Pi** |
| Flats, guider on a real sky, tracking on real H-alpha images, prominence tools, safe eject, USB recovery, iPhone app, HTTPS on the phone | Built and tested on the simulators; **waiting for a real-hardware check** ([checklist](docs/project/hardware-checklist.md)) |
| ZWO ASI cameras (e.g. ASI174MM Mini) | Driver built into the package (loads `libasi` at run time); **waiting for a real camera on a USB 3 cable** |

See [docs/project/roadmap.md](docs/project/roadmap.md) for the plan and milestones.

## Quick try (no hardware)

```sh
cd web && npm install && npm run build && cd ..
cargo run --release -p rustflare-server        # http://localhost:8080
```

In the UI: open the *simulated camera*, connect the *simulator* mount, enable solar tracking, start tracking, calibrate, centre. `scripts/smoke.sh` does the same through the API.

## Documentation

Start at [docs/index.md](docs/index.md) (`mkdocs serve` renders it): a user guide, a reference and developer material. Design decisions are in [docs/adr](docs/adr/README.md).

## Security

By default there is **no password** and the server can slew a mount: use a trusted network, or set a password and enable HTTPS ([docs/guide/phone-setup.md](docs/guide/phone-setup.md)). It is built for a telescope on a LAN, not for exposure to the internet.
