# Working on RustFlare (for coding agents and new contributors)

RustFlare is a Rust + React solar-capture server for a Raspberry Pi: a web UI and REST API in front of a camera, a mount, a Sun tracker and guider, and a raw recorder. Read [docs/project/architecture.md](docs/project/architecture.md) and [docs/project/development.md](docs/project/development.md) first; decisions are in [docs/adr/](docs/adr/).

## Build and test (all of this runs without hardware)

```sh
cargo test --workspace                            # unit tests, mock-I/O protocol tests, closed-loop guiding tests
cargo build --release -p rustflare-server
cd web && npm ci && npm run build                 # type-check + production build
PLAYWRIGHT_BROWSERS_PATH=<dir> npx playwright test   # 34 browser tests against the simulators (needs the release server built above)
scripts/smoke.sh ./target/release/rustflare-server ./data-smoke
scripts/sim/<scenario>.sh                         # multi-step scenarios on the simulators, see scripts/sim/README.md
```

CI (`.github/workflows/ci.yml`) runs the unit tests, the web build, the smoke test, the Playwright tests and the arm64 package build. A change is not done until those pass.

## Rules that are easy to break

- **One engine thread owns the hardware** (`crates/server/src/engine.rs`). Handlers send `Command`s; nothing else touches the camera or mount. Status goes out through the watch channel.
- **One model of where the Sun is** (`crates/server/src/sunmodel.rs`). The Sun outline, the tracking circle, the coronagraph mask and the guider all read it. Never feed a second estimator into the UI or the mask.
- **Never act on a single measurement.** Shadows and cloud made the detector jump by hundreds of pixels; anything that moves the mount or places a mask goes through the model or the guider's gating.
- **Raw data is the product.** Recordings are raw SER/FITS. Processing (debayer, masks, stretch) is display-only except the optional coronagraph lane, which says so in its notes.
- **Drivers are traits** in `rustflare-core` (`Camera`, `CameraDriver`, `Mount`). A driver with native dependencies sits behind a Cargo feature; the default build must work on the simulators alone. The UI renders whatever controls a driver reports.
- **Do not commit** recordings, session logs, site coordinates, camera serial numbers, Bluetooth addresses or tokens. Keep fixtures synthetic or anonymised and small.
- A new cross-cutting decision gets an ADR. User-facing behaviour changes update `docs/guide/`; behaviour of the API updates `docs/reference/api.md`.

## Hardware facts that cost hours to find

- **ZWO AM3 guide commands last 3 seconds at most**, whatever you ask for (`:Mge3000#` style). Hold a longer move by re-issuing it every 2.5 s. Calibration and guiding both do (`crates/tracking/src/guiding.rs`, `MAX_PULSE_CHUNK`).
- **`:GAST#` answers `date&time&offset#RA&Dec#flags#`**: three '#'-terminated parts. Read all three or the pointing is lost.
- The mount's `:GU#` flags turn lower case (`nNGM…`) when tracking is off; on the AM3 it stops tracking about 19 minutes past the meridian unless flipped. A Go to Sun past the meridian flips the mount, and the image then turns by about 180 degrees: recalibrate.
- Over Bluetooth every command takes about 60 ms; move commands have no reply. See `docs/project/zwo-ble-protocol.md`.
- FAT32 sticks: files split before 4 GB (`rustflare-capture`).
- Light under trees or cloud can drop a hundredfold in seconds. The detector brightens dim frames after removing the black level, uses the sky edge as the limb and filters limb points with RANSAC.
- Lane recordings change the exposure every few seconds; only the shortest non-masked lane is measured, and the guider judges once per burst.

## Debugging a real system

Logs are on the capture drive under `logs/<date>/` and readable over HTTP: see [docs/reference/debugging.md](docs/reference/debugging.md). The useful ones: `GET /api/debug/events`, `GET /api/debug/mount` (every mount command and reply), `GET /api/debug/state`, `POST /api/debug/snapshot`, and the lost-tracking frames. Deploy to a Pi with `scripts/pi-deploy.sh user@host` (it refuses while a recording runs).

## Picking up work

Open issues are the backlog; the `hardware-verification` label marks things built and simulator-tested that need a real device, and `docs/project/hardware-checklist.md` says how to check each. `good first issue` and `help wanted` are meant for outsiders. Say what you could not test: "tested on simulators only" is a normal and welcome statement.
