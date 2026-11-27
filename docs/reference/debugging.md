# Debugging and logs

The logs can be read over the HTTP API, so a problem can be investigated remotely while the Sun is still up. The small always-on logs (events and mount commands) live on the **Pi's own disk** (`/var/lib/rustflare/logs/<date>/`), so they survive an ejected drive; they are kept to 200 MB and 14 days, oldest days deleted first. The bigger ones (lost frames, session logs, snapshots) go on the capture drive under `logs/<date>/`.

| What | Where | Always on? |
|---|---|---|
| **Events**: tracking lost/found, guiding start/stop/resume, dead reckoning, calibration, exposure changes, lane switches, capture start/end/abort/resume, flips, the Sun model holding or measuring again | `events.jsonl`, `GET /api/debug/events` | yes, on the Pi's disk (20 MB a day, 200 MB and 14 days in total) |
| **Mount commands**: every command sent to the mount and its reply, with the time it took (every mount driver writes here: ZWO and iOptron alike, in the mount's own command language) | `mount.jsonl`, `GET /api/debug/mount` | yes, on the Pi's disk (40 MB a day, within the same total) |
| **Lost-tracking frames**: the full frame the tracker could not use, plus a note (exposure, gain, lane, mask, last Sun position, engine state) | `lost/lost_<time>.png` and `.json` | yes, at most one every 10 s and 100 per run; switch under More > Session log |
| **Session log**: once a second what the tracker and guider saw and the pulses sent, plus a small picture every few seconds | `session_<time>.jsonl` and `_fNNNNN.png` | opt in (More > Session log) |
| **Snapshots**: the latest full frame and a note on request | `debug/debug_<time>.png` and `.json` | on request: `POST /api/debug/snapshot` |

## Reading them

- *The Sun jumped.* In `events.jsonl` look for `sun_predicted` (the model refused measurements for a while) and `tracking_lost`. In `mount.jsonl` look at the commands just before: if there is none, the mount moved by itself (mechanical, firmware, or the mount's own limit). A `:GU#` reply whose first letter turns lower case (`nNGM…`) means the mount stopped tracking.
- *The mask is in the wrong place in a prominence lane.* The model is only measured on the disc lane; compare `sun_for_mask` in `GET /api/debug/state` with a snapshot.
- *The flip did nothing.* Look for `auto_flip_recorded` or `auto_flip_record_failed` in the events. The status bar shows the same reason.
- Mount replies of a few letters are normal: guide and slew commands have no reply, `:GU#` returns the status flags, `:GAST#` returns `date#RA&Dec#flags#`.

## Replaying the detector

Copy the `logs` folder off the drive and run the detector on the pictures:

```sh
RUSTFLARE_THUMBS=<folder of png files> RUSTFLARE_EXPECTED_R=<radius in px> cargo test -p rustflare-server thumbs_report -- --nocapture
RUSTFLARE_FIXTURES=<logs folder> cargo test -p rustflare-server replay_real -- --nocapture
```

Nothing identifying is logged: no site, device names or file paths. The mount log holds only mount commands, which include the latitude and longitude only when a Go to Sun or a flip sends them to the mount.

The stacker's own log, `stack.jsonl`, is in the same day folders (limits it was given, every job with CPU, throttling, temperature and what the recorder was doing, state changes with reasons). Summarise it with `scripts/analysis/stack_telemetry.py`; see [Stacking](../guide/stacking.md#telemetry-how-it-behaves-next-to-a-recording).
