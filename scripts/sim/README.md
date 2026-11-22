# Scenario scripts (simulators, no hardware)

Each script starts its own server on the simulators, drives it through the real HTTP API and prints what happened. They exist because the interesting bugs (shadows, clouds, lane recordings, flips) only show up when several parts run together over time. Build first:

```sh
cargo build --release -p rustflare-server
(cd web && npm ci && npm run build)
scripts/sim/<name>.sh
```

| Script | What it shows |
|---|---|
| `lost-target.sh` | the Sun leaves the frame while guiding: guiding pauses, resumes when the limb is back, the lost frame is saved |
| `dead-reckoning.sh` | guiding learns the drift, the Sun is hidden for 50 s, the framing is still within a few pixels afterwards |
| `lanes.sh`, `lanes-stop.sh` | HDR / prominence lanes: folders, exposure restored, stopping at any moment leaves the camera as it was |
| `lanes-guide.sh` | guiding while a lane recording runs: the Sun stays near the target (it drifted away before the lane-mode guider) |
| `lane-follow.sh` | the Sun model follows the Sun while the mount moves during a lane recording |
| `flip.sh` | the meridian flip assist: record, GoTo, re-centre |
| `autoflip.sh`, `autoflip-resume.sh` | the automatic flip, with guiding (recalibrates) and with a running lane recording (pauses, then resumes) |
| `focus.sh` | focus assist: sharp scores higher than defocused |
| `mask.sh` | the coronagraph mask only affects the prominence lane |

The simulators (`rustflare-core::sim`, `rustflare-camera::simulator`, `rustflare-mount::simulator`) have tunable drift and a defocus and zoom control; the mock-I/O tests for the real protocols are in the mount crate.
