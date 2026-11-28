# Architecture Decision Records

Decisions that shape RustFlare, in [MADR](https://adr.github.io/madr/)-style short form.
Add a new record by copying [`template.md`](template.md) to the next number. Never rewrite
an accepted record: supersede it with a new one and link both ways.

| # | Title | Status |
|---|-------|--------|
| [0001](0001-record-architecture-decisions.md) | Record architecture decisions | Accepted |
| [0002](0002-rust-workspace-and-vidoxide-reuse.md) | Rust workspace; port (not fork) Vidoxide code | Accepted |
| [0003](0003-web-ui-react-vite.md) | Web UI: React + Vite + TypeScript, served by the Rust binary | Accepted |
| [0004](0004-engine-actor-and-transport.md) | Single engine thread, REST + WebSocket + MJPEG | Accepted |
| [0005](0005-hardware-abstraction.md) | `Camera` / `Mount` traits, drivers, simulators | Accepted |
| [0006](0006-playerone-via-vendor-sdk.md) | Player One cameras via the vendor SDK, not V4L2 | Accepted (verified on a real Apollo-M MINI) |
| [0007](0007-zwo-mount-serial-driver.md) | ZWO mount: direct serial protocol, solar rate, guide-rate fix | Accepted (run on a real AM3) |
| [0008](0008-solar-tracking-and-guiding.md) | Tracking modes, automatic calibration, pulse guiding | Accepted |
| [0009](0009-capture-formats-and-flats.md) | SER for sequences, PNG for stills, automatic flats | Accepted |
| [0010](0010-debian-package-and-updates.md) | Debian package, GitHub Releases (the updater part is superseded by 0017) | Accepted |
| [0011](0011-storage-selection-and-safety.md) | Storage selection and safety (USB drives, FAT32) | Accepted |
| [0012](0012-burst-capture-and-ram-buffer.md) | Burst capture through a RAM buffer | Accepted |
| [0013](0013-partial-disc-first-tracking-and-optics.md) | Partial-disc-first tracking, Otsu detection, optics | Accepted |
| [0014](0014-zwo-asi-cameras.md) | ZWO ASI cameras via a run-time loaded vendor library | Accepted |
| [0015](0015-ble-mount-transport.md) | Bluetooth LE transport for the ZWO mount | Accepted (verified on a real AM3) |
| [0016](0016-optional-https-and-password.md) | Optional HTTPS (private CA) and password | Accepted |
| [0017](0017-no-updater.md) | No updater: debs and the signed apt repository | Accepted |
| [0018](0018-storage-by-uuid.md) | Remember the capture drive by its UUID | Accepted |
| [0019](0019-shared-frame-ring.md) | One shared frame ring for capture, raw writing and stacking | Accepted |
| [0020](0020-ioptron-mount-driver.md) | iOptron mount driver (CEM70 series, HAE69C) | Accepted (a real HAE69C connected; guiding and Go to Sun not yet) |
| [0021](0021-use-profiles.md) | Use profiles: what is imaged, grey out with a reason, never hide | Accepted (simulators only) |
| [0022](0022-fixed-view-and-overlay-chips.md) | A fixed view: the page never scrolls, messages sit on the picture | Accepted |
| [0023](0023-mount-site-and-time-sync.md) | The mount's own site and time: read back, compare, send | Accepted |
| [0024](0024-file-layout-and-calibration-catalog.md) | A self-describing file layout, a calibration library per camera and a catalog per folder | Accepted (step 1 built) |
