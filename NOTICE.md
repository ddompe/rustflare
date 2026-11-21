# Notice: third-party code and ideas

## Vidoxide (MIT)

<https://github.com/GreatAttractor/vidoxide> — Copyright (c) 2020-2024 Filip Szczerek. Licence text: `LICENSE.vidoxide`.

| RustFlare file | Derived from (Vidoxide `src/`) | What changed |
|---|---|---|
| `crates/mount/src/zwo.rs` | `mount/zwo.rs` (also © 2024 Diego Dompe) | Generic I/O + unit tests; guide-rate clamp fix; solar/lunar/sidereal rate; `stop()` keeps tracking (ADR 0007) |
| `crates/core/src/mount.rs` | `mount/mod.rs` | `RadPerSec`, `SlewSpeed`, `Axis`, sidereal constants; trait reshaped |
| `crates/tracking/src/tracker.rs` | `tracking.rs` | Anchor tracker ported to `Frame`; full-disc and limb trackers are new |
| `crates/tracking/src/guiding.rs` | `guiding.rs` | Direction maths and guide loop kept; timing restructured as pure state machines; automatic calibration is new |
| `crates/capture/src/ser.rs` | `output/ser.rs` | Timestamp trailer, standard signature, tests |
| `crates/core/src/camera.rs` | `camera/mod.rs` | Control model simplified to a list of `ControlDesc` |

## Other references (no code copied)

- INDIGO `indigo_mount_asi.c` (<https://github.com/indigo-astronomy/indigo>) — consulted to confirm the ZWO `:TS#`/`:TQ#`/`:TL#` commands and the 0.1–0.9 guide-rate range.
- `playerone-sdk` / `playerone-sdk-sys` crates — used as dependencies. `playerone-sdk-sys` bundles Player One's proprietary `libPlayerOneCamera`; check its licence before redistributing binaries.
