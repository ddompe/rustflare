# Rustflare

Headless, web-controlled **solar capture** in Rust. Run it on a Raspberry Pi next to the telescope, open a web page from anywhere on your network, and:

- see a live view with a tracking overlay,
- control a **Player One** camera (exposure, gain, offset, 8/16-bit),
- control a **ZWO AM3/AM5** mount (solar tracking rate, slewing, guiding),
- keep the Sun in frame by tracking the **full disc**, a **partial disc** (fitted from the visible limb), or a **surface feature** such as a sunspot, with automatic mount calibration and guiding,
- capture **single frames** (PNG), **sequences and scheduled bursts** ("100 frames every 30 s", SER or PNG images, through a RAM buffer so slow disks don't limit fast cameras) and **flats** (automatic exposure, SER + master flat),
- choose a **USB drive** for captures in the UI, with free-space and FAT32 safeguards,
- see it all in a live view with **centring aids** (bullseye, Sun outline), zoom, invert / H-alpha tint, auto-stretch and a histogram.

Based on ideas and code from [Vidoxide](https://github.com/GreatAttractor/vidoxide) (MIT) — see [NOTICE.md](NOTICE.md).

## Install on a Raspberry Pi

```sh
# from the latest release: https://github.com/ddompe/rustflare/releases/latest
sudo apt install ./rustflare_*_arm64.deb
sudo rustflare-update          # later: install the newest release
```

Then open `http://<pi-name>.local:8080/`. Details in [docs/getting-started.md](docs/getting-started.md).

## Status

| Area | State |
|---|---|
| Server, API, web UI, tracking (limb / feature / disc), calibration, guiding, bursts + RAM buffer, flats, storage selection | Working end-to-end **against the built-in simulators**, also on a real Raspberry Pi 5; unit tests for all pure logic |
| Debian package, install, upgrade, `rustflare-update` | Verified on a Pi 5; CI/release workflows not yet run on GitHub |
| Player One camera driver | Library loads and enumerates on the Pi; **not yet run with a real camera** |
| ZWO AM3/AM5 mount driver | Ported from Vidoxide with fixes; unit-tested on a mock port; **not yet run on a real mount in this project** |
| Tracking on real H-alpha images | Tested on synthetic frames only |

See [docs/roadmap.md](docs/roadmap.md) for what to verify first on real hardware and what is planned (including a user guide).

## Quick try (no hardware)

```sh
cd web && npm install && npm run build && cd ..
cargo run --release -p rustflare-server        # http://localhost:8080
```

In the UI: open the *simulated camera*, connect the *simulator* mount, enable solar tracking, start tracking, calibrate, centre. `scripts/smoke.sh` does the same through the API.

## Documentation

Start at [docs/index.md](docs/index.md) (`mkdocs serve` renders it). Design decisions are in [docs/adr](docs/adr/README.md).

## Security

There is **no authentication**. The server can slew a mount. Run it only on a trusted network (or behind a VPN / reverse proxy with auth).
