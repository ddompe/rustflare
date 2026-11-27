# Working on RustFlare (for coding agents and new contributors)

RustFlare is a Rust + React solar, lunar and planetary capture server for a Raspberry Pi: a web UI and REST API in front of a camera, a mount (ZWO AM3 family, iOptron CEM70 series and HAE69C, simulator), a Sun (or Moon, or planet) tracker and guider, a raw recorder and a separate stacker. Read [docs/project/architecture.md](docs/project/architecture.md) and [docs/project/development.md](docs/project/development.md) first; decisions are in [docs/adr/](docs/adr/).

## Build and test (all of this runs without hardware)

```sh
cargo test --workspace                            # unit tests, mock-I/O protocol tests, closed-loop guiding tests
cargo build --release -p rustflare-server
cd web && npm ci && npm run build                 # type-check + production build
PLAYWRIGHT_BROWSERS_PATH=<dir> npx playwright test   # about 120 test definitions, each run on a phone and a desktop viewport (about 320 runs), against the simulators (needs the release server built above)
scripts/smoke.sh ./target/release/rustflare-server ./data-smoke
scripts/sim/<scenario>.sh                         # multi-step scenarios on the simulators, see scripts/sim/README.md
```

CI (`.github/workflows/ci.yml`) runs the unit tests, the web build, the smoke test, the Playwright tests and the arm64 package build. A change is not done until those pass. A release is a pushed tag `vX.Y.Z` (`release.yml`); `scripts/pi-deploy.sh` installs a **local build** (version `X.Y.Z+dev...`, shown as `(local <commit>)` in the status bar) and is not a release.

## Rules that are easy to break

- **One engine thread owns the hardware** (`crates/server/src/engine.rs`). Handlers send `Command`s; nothing else touches the camera or mount. Status goes out through the watch channel.
- **One model of where the Sun is** (`crates/server/src/sunmodel.rs`). The Sun outline, the tracking circle, the coronagraph mask and the guider all read it. Never feed a second estimator into the UI or the mask.
- **Never act on a single measurement.** Shadows and cloud made the detector jump by hundreds of pixels; anything that moves the mount or places a mask goes through the model or the guider's gating.
- **Raw data is the product.** Recordings are raw SER/FITS. Processing (debayer, masks, stretch) is display-only except the optional coronagraph lane, which says so in its notes.
- **Profiles adapt the screen, they do not lock it** ([ADR 0021](docs/adr/0021-use-profiles.md)): a control that does not apply to the profile (`sun_white`, `sun_ha`, `moon`, `planet`, `other`) is greyed with its reason written under it, never hidden, and the API refuses nothing because of the profile.
- **The screen is fixed** ([ADR 0022](docs/adr/0022-fixed-view-and-overlay-chips.md)): the page never scrolls, the picture keeps its size, and anything that comes and goes is a chip on the picture, not a line that moves the controls. UI strings are checked by the browser tests; when you rename one, update `docs/guide/`.
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

- **iOptron HAE69C (model code `0066`):** replies are slow, some 300 to 600 ms (`:ST1#` tracking, `:qR#` stop), so the driver waits 2 s for a reply and drops stray input before each command; read-backs run every 30 s, not with the 2 s poll. It has **no clock battery** and restarts from 2017-01-01 14:00 UTC, so the Pi sends its site and time once after connecting ([ADR 0023](docs/adr/0023-mount-site-and-time-sync.md)). *Search home* (`:MSH#`) makes the found position the new zero: it is not *Send home*. A ZWO mount reads its clock only.
- **ZWO ASI533MC cooler:** the fan and anti-dew switches exist only where the SDK lists them; switching a cold sensor off raises the target 1 degree every 15 s first (`cooler.rs`), `immediate` skips it. Not yet run on a real cooled camera.
- **USB on the Pi 5:** a USB 3 camera and a USB 3 drive together made both drop off the bus under load; the drive belongs on a black USB 2.0 port. A drive pulled without ejecting comes back read-only. The engine finds a re-enumerated mount again by itself (a camera that drops stops the capture without a message, issue #95).

## Debugging a real system

Logs are on the capture drive under `logs/<date>/` and readable over HTTP: see [docs/reference/debugging.md](docs/reference/debugging.md). The useful ones: `GET /api/debug/events`, `GET /api/debug/mount` (every mount command and reply), `GET /api/debug/state`, `POST /api/debug/snapshot`, and the lost-tracking frames. Deploy to a Pi with `scripts/pi-deploy.sh user@host` (it refuses while a recording runs).

## Picking up work

Open issues are the backlog; the `hardware-verification` label marks things built and simulator-tested that need a real device, and `docs/project/hardware-checklist.md` says how to check each. `good first issue` and `help wanted` are meant for outsiders. Say what you could not test: "tested on simulators only" is a normal and welcome statement.
