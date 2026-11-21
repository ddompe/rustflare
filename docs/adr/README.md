# Architecture Decision Records

Decisions that shape Rustflare, in [MADR](https://adr.github.io/madr/)-style short form.
Add a new record by copying [`template.md`](template.md) to the next number. Never rewrite
an accepted record: supersede it with a new one and link both ways.

| # | Title | Status |
|---|-------|--------|
| [0001](0001-record-architecture-decisions.md) | Record architecture decisions | Accepted |
| [0002](0002-rust-workspace-and-vidoxide-reuse.md) | Rust workspace; port (not fork) Vidoxide code | Accepted |
| [0003](0003-web-ui-react-vite.md) | Web UI: React + Vite + TypeScript, served by the Rust binary | Accepted |
| [0004](0004-engine-actor-and-transport.md) | Single engine thread, REST + WebSocket + MJPEG | Accepted |
| [0005](0005-hardware-abstraction.md) | `Camera` / `Mount` traits, drivers, simulators | Accepted |
| [0006](0006-playerone-via-vendor-sdk.md) | Player One cameras via the vendor SDK, not V4L2 | Accepted (unverified on hardware) |
| [0007](0007-zwo-mount-serial-driver.md) | ZWO mount: direct serial protocol, solar rate, guide-rate fix | Accepted (unverified on hardware) |
| [0008](0008-solar-tracking-and-guiding.md) | Tracking modes, automatic calibration, pulse guiding | Accepted |
| [0009](0009-capture-formats-and-flats.md) | SER for sequences, PNG for stills, automatic flats | Accepted |
| [0010](0010-debian-package-and-updates.md) | Debian package, GitHub Releases, `rustflare-update` | Accepted (CI unverified) |
| [0011](0011-storage-selection-and-safety.md) | Storage selection and safety (USB drives, FAT32) | Accepted |
| [0012](0012-burst-capture-and-ram-buffer.md) | Burst capture through a RAM buffer | Accepted |
| [0013](0013-partial-disc-first-tracking-and-optics.md) | Partial-disc-first tracking, Otsu detection, optics | Accepted |
