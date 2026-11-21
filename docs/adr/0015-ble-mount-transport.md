# 0015. Bluetooth LE transport for the ZWO mount

- Status: Accepted (implemented; not yet verified against a live mount, see Consequences)
- Date: 2026-11-21

## Context

Issue #53 asks for wireless mount control. The AM3/AM5 speaks the same ASCII `:...#` protocol over Bluetooth LE as over USB, through one GATT characteristic (write with response in, notifications out) after a session opener; see [ZWO mount Bluetooth protocol](../zwo-ble-protocol.md). The Pi runs BlueZ. RustFlare is cross-compiled from a Mac with `cargo-zigbuild` and no target libraries, so a C library such as `libdbus` or `libbluetooth` is not available at build time.

## Decision

- **Transport, not a new driver.** `rustflare_mount::ble::BleTransport` implements `Read + Write + Send` on top of a small `Gatt` trait; the existing `Zwo<P>` driver runs on it unchanged. `write` is one ATT Write Request; notifications are queued and handed out byte by byte; the read timeout is 1.5 s (the serial port's 50 ms is shorter than a BLE round trip). Late notifications are dropped before each write so a reply cannot be matched to the wrong command.
- **Session opener** `:BS<token>#` -> `OK#` runs once after notifications are enabled. The value is a documented constant, overridable with the settings key `zwo_ble_token` or the environment variable `RUSTFLARE_ZWO_BLE_TOKEN` (the environment wins; an empty or non-numeric override is an error).
- **BlueZ backend: `zbus` (pure Rust) talking to `org.bluez`.** Alternatives: `bluer` (the official BlueZ crate, but it needs `libdbus` through `dbus-rs`, which we cannot link when cross-compiling), `btleplug` (same dependency chain on Linux), `dbus` with a vendored libdbus (needs a C toolchain per target in zigbuild; fragile). `zbus` builds with plain `cargo zigbuild` and adds no run-time library beyond `bluetoothd` itself. The cost: we write the few BlueZ calls ourselves (`StartDiscovery`, `Device1.Connect`, `StartNotify`, `WriteValue`, `PropertiesChanged`), about 250 lines.
- **Cargo feature `ble`** (crate `rustflare-mount`, forwarded by `rustflare-server`), enabled by `scripts/build-deb.sh`; the default build and CI do not pull in zbus. The package `Recommends: bluez`.
- **No pairing, no trust.** The backend never pairs or trusts the device and calls `Adapter1.RemoveDevice` on disconnect, so nothing stays in BlueZ.
- **Device selection.** `GET /api/mount/scan` lists devices named `AM3*`/`AM5*` or advertising service `000000ff-...` (address, name, RSSI; takes about 6 s). `POST /api/mount/connect` accepts `{"driver":"zwo","transport":"ble","device":"<address>"}`; the USB form is unchanged. The startup settings keep the same `(driver, device)` pair, with the device stored as `ble:<ADDRESS>`, so old settings files keep working.
- **Engine safety.** The driver reports `link()` (`usb`/`ble` and whether it is up; `status.mount.link`, `link_up`). A write failure or a `Connected=false` signal marks the link lost; the engine then stops guiding, sends a best-effort stop, drops the mount, shows an error and keeps trying to reconnect for 10 minutes. Auto-reconnect over Bluetooth runs on its own thread (the mount may not be advertising for a long time) so the engine loop never waits on it; every D-Bus call has an 8 s timeout. An explicit connect from the UI blocks the engine for up to about 25 s (scan 12 s, connect, service discovery, opener). Whether the mount stops by itself when the link dies is **not known**: a slew started from RustFlare is a rate move, so a lost link cannot always be stopped from our side.

## Consequences

- Unit tests cover framing, chunked notifications, timeouts, opener success and failure, stale replies, write failure and link drop, and the Zwo driver on top, all with a fake GATT backend. The BlueZ backend itself (D-Bus calls, signal handling) compiles for `aarch64-unknown-linux-gnu` but has **not been run against a real mount**; the first live session is the test. Open points: MTU for long writes (our commands are at most about 30 bytes), whether `:Rg`/`:Mg` guiding pulses and `:R`/`:M` behave over BLE as over USB, and the scan filter on a mount that is only briefly advertising.
- Only one central can hold the mount: the phone app must be closed.
