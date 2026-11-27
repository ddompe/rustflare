# Contributing

Patches are welcome, especially **support for more cameras and mounts**. This is a hobby project built around one maintainer's equipment (Lunt LS50, ZWO AM3, an iOptron HAE69C, a Player One Apollo-M Mini, a ZWO ASI174MM Mini, Raspberry Pi 5), so a lot of it is verified on that kit only and the rest on simulators. Say clearly what you tested on what.

## Adding equipment

1. Read [Adding a camera or mount](docs/project/development.md#adding-a-camera-or-mount) and the matching ADR ([5](docs/adr/0005-hardware-abstraction.md), [6](docs/adr/0006-playerone-via-vendor-sdk.md), [7](docs/adr/0007-zwo-mount-serial-driver.md), [14](docs/adr/0014-zwo-asi-cameras.md), [15](docs/adr/0015-ble-mount-transport.md), [20](docs/adr/0020-ioptron-mount-driver.md)).
2. Implement `Camera`/`CameraDriver` or `Mount` from `rustflare-core` in a new file under `crates/camera/src/` or `crates/mount/src/`. Report settings as controls with the standard ids so flats and the UI work unchanged.
3. Put native libraries behind a Cargo feature; the default build must work without them.
4. Add a mock-I/O or simulator test. The mount crate has a mock serial port; every protocol bug found so far was found with a transcript of the real device, so please include one in the test.
5. Open the pull request with what hardware you used (or "simulator only").

## Any change

- `cargo test --workspace`, `cd web && npm run build`, and the Playwright tests must pass (CI runs them).
- Update the docs that describe the behaviour (`docs/guide/` for users, `docs/reference/` for the API and internals). Keep planning and status notes out of those two parts.
- Keep decisions in an ADR when they cross crates.
- No personal data in the repository: recordings, site coordinates, serial numbers, addresses, tokens.
- AI-assisted changes are fine, as the project itself is largely built that way; the rules above apply the same.

## Hardware verification

Things that are built and tested on the simulators but not on a real device carry the `hardware-verification` label and a row in [docs/project/hardware-checklist.md](docs/project/hardware-checklist.md). A report of "I ran step N on this equipment and saw X" is a real contribution. Logs from `GET /api/debug/events` and `GET /api/debug/mount` make such reports far more useful.

## Safety

The software moves a real mount and points a camera at the Sun. It does not protect your eyes or your sensor: use a proper solar filter, stay at the scope with cables clear, and read the disclaimers in the README.
