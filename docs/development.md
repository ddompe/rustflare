# Development

## Layout

```
crates/     core camera mount tracking capture server
web/        React + Vite UI (src/, components/)
docs/       this documentation; docs/adr/ decision records
packaging/  files of the .deb: systemd unit, udev rule, maintainer scripts, rustflare-update, rustflare-user
scripts/    build-deb.sh, smoke.sh, mock-github-release.py
.github/    CI (ci.yml) and release (release.yml) workflows
```

## Everyday commands

```sh
cargo test --workspace                       # unit tests (no hardware)
cargo build --release -p rustflare-server
scripts/smoke.sh ./target/release/rustflare-server ./data-smoke   # e2e on simulators

# cross-compile the Raspberry Pi package (arm64 .deb) on any machine:
#   rustup target add aarch64-unknown-linux-gnu
#   cargo install --locked cargo-zigbuild cargo-deb      (+ zig: https://ziglang.org/download/)
scripts/build-deb.sh                      # -> dist/rustflare_<version>-1_arm64.deb, dist/SHA256SUMS
scp dist/rustflare_*_arm64.deb pi@<pi>:/tmp/ && ssh pi@<pi> 'sudo apt install -y /tmp/rustflare_*_arm64.deb'
SMOKE_URL=http://<pi>:8080 scripts/smoke.sh     # end-to-end test against the running Pi

cd web && npm install
npm run dev          # UI on :5173, proxies to a server on :8080 (RUSTFLARE_URL to change)
npm run build        # type-check + production build into web/dist
```

## Tests

Pure logic is where the tests are: ZWO protocol (mock serial port), trackers (synthetic frames, including a partially visible disc),
calibration and guiding (closed loop against a physical model), H-alpha-like disc detection, SER layout, the asynchronous frame writer (buffering, FAT32 rollover, PNG folders, write errors), storage selection, flat-exposure convergence, simulator behaviour.
`scripts/smoke.sh` drives calibration, guiding, single/sequence/flats through the real HTTP API.
Not covered by automated tests: real cameras and mounts, the web UI's behaviour (it is type-checked and was checked by eye once).

## Releasing

1. Bump `version` under `[workspace.package]` in `Cargo.toml`, commit.
2. `git tag vX.Y.Z && git push origin vX.Y.Z`. `release.yml` checks the tag equals the Cargo version, builds the arm64 package and publishes the release with `SHA256SUMS`.
3. On the Pi: `sudo rustflare-update`.

Test the update flow without GitHub: `scripts/mock-github-release.py --deb <deb> --sums SHA256SUMS --port 9999`, then `sudo env RUSTFLARE_API_URL=http://127.0.0.1:9999 rustflare-update`. (Verified this way: check, tampered-checksum abort, upgrade, already-up-to-date, config and captures preserved.)

## Conventions

- New cross-cutting decisions get an ADR.
- Hardware access only through the engine thread.
- Driver features with native dependencies go behind Cargo features; the default build must work with the simulator alone.
- Keep upstream attribution current in `NOTICE.md` when porting more Vidoxide code.

## Building on a machine with a small system disk

The toolchain can live anywhere: set `RUSTUP_HOME` / `CARGO_HOME` (and `npm_config_cache`) to a folder on a large disk before installing. Zig and the Chrome profile (if you take headless screenshots) can live there too.

## Real-data regression fixtures (session log)

*More > Session log* records, once a second, what the software saw and did (camera level, detected Sun, tracking and guiding state, guide pulses) plus a small grayscale picture every N seconds, into `<capture drive>/logs/<date>/session_<time>.jsonl` and `..._f00001.png`. Nothing identifying is logged and it stops at 200 MB.

To use a real session as a regression test: copy the `logs` folder off the drive and run

```
RUSTFLARE_FIXTURES=/absolute/path/to/logs cargo test -p rustflare-server replay_real -- --nocapture
```

It runs the Sun detector over every logged picture (at its thumbnail scale) and fails if a Sun that was found during the session is lost now or its centre moved by more than 12 full-resolution pixels. Keep real recordings out of the repository (they show your site and equipment); commit only synthetic or deliberately anonymised, small fixtures.
