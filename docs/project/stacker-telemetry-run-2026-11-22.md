# Stacker telemetry run, 2026-11-22

The first run of the stacker service on the Pi 5 with its telemetry, using the field day's real recordings and a recording going on at the same time. The raw telemetry is `stack.jsonl` in the Pi's logs; the tool is `scripts/analysis/stack_telemetry.py`.

## Setup

- Pi 5, 8 GB, kernel 6.18.50+rpt-rpi-2712; the 233 GB FAT32 stick; the packaged service (`nice 19`, `CPUWeight 10`, `CPUQuota 200 %`, idle I/O class, I/O weight 10, 2 GB memory limit, allowed on cores 0 to 3, run as the recorder's account).
- Work: 12 real bursts of 1944x1472 (6 disc, 6 prominence, 572 MB each) from the 17:48 session, made pending by adding `.ready` markers (removed afterwards), plus the stacker's own simulated recordings.
- Parallel capture: the recorder on the same Pi recording with the **simulator** camera to the same stick, bursts of 100 frames (1280x960, 16-bit, about 7 fps on the Pi), no camera or mount attached.
- Phases: **A** recording alone (stacker off); **B** stacker on `between_bursts` (minimum gap 8 s), bursts every 20 s; **C** stacker on `always`, bursts every 8 s (the recording is then nearly continuous).

## What it measured

| | |
|---|---|
| Real disc burst, nothing else running | 8.8 to 9.9 s wall, 1.6 cores used of the 2 allowed |
| Real prominence burst, nothing else running | 7.3 to 8.8 s wall, 1.5 cores |
| CPU throttling by the quota | 0.01 to 0.04 s per job (a few periods in 8 s): the quota is not the limit, the work is |
| System load during a job | 60 to 63 % averaged over the 4 cores (one core idle), threads moving across all four cores |
| Temperature, clock | 70 C at most, 2400 MHz throughout, firmware throttle flags never set |
| Memory and I/O pressure (PSI) | zero stall time in every job |
| One real disc burst stacked *while* a burst was being recorded (`always`) | 11.8 s against 8.9 s alone (+33 %), 1.2 cores |
| `between_bursts` with 14 s bursts every 20 s | the 6 s gaps are shorter than the 8 s minimum, so it waited until the recording ended: as designed |
| Recorder, frames dropped | 0 in every phase |
| Recorder, frame rate | median 8.1 fps alone, 7.8 with the stacker between bursts, 7.7 with `always` (about 5 % lower, within the simulator's own variation) |
| Recorder, RAM buffer high-water mark | 1 frame alone, 0 between bursts, 83 frames with `always` at the faster cadence (drained, never full: 2231 frames of room) |

## What it means, and what it does not

- The service holds to its limits and the machine stays cool and unthrottled: the CPU quota, nice 19 and the idle I/O class behaved.
- Stacking while a burst is written costs the stacker about a third more time and made no measurable difference to the recorder here.
- **This is not the real test.** The simulated camera writes about 17 MB/s at 7 fps; the Apollo-M Mini writes up to 325 MB/s at 57 fps and drives the USB bus and its interrupts hard. Whether the stacker disturbs *that* is exactly what is still open (#86 capture stalls). The telemetry is there to answer it: run the real camera with `between_bursts`, then with `always`, and compare the recorder's stall counters with the job table.
- Observations to try on the real camera: the stacker's threads wander over all four cores, so they may share a core with the camera's USB interrupt handling. `sudo systemctl edit rustflare-stack` with `[Service]` and `AllowedCPUs=2-3` would keep it off cores 0 and 1; the telemetry (`core_busy_pct`, `cores_seen_running`) shows the effect. Rust sees 2 CPUs under the 200 % quota, so the stacker's own thread count follows the quota.
- Not covered: a recording of hours, the SD card instead of the stick, and a second Pi load such as the hotspot or the web page streaming at the same time.
