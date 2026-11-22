# Architecture

```
 Browser (React) ──REST──► axum handlers ──Command──►  ENGINE THREAD
     ▲  ▲                                                │ owns:
     │  └── WebSocket: Status (watch channel) ◄───────────┤  Camera (blocking capture)
     └───── MJPEG /stream.mjpg ◄── preview thread ◄──────┤  Mount
                          (1-slot, drops frames)         │  Tracker → Guider/Calibrator
                                                         │  Capture job (single / bursts / flats)
                                                         │      └─► RAM buffer ─► writer thread ─► files
                                                         └─  Storage (chosen drive, free space, safety)
```

Crates:

| Crate | Role |
|---|---|
| `rustflare-core` | `Frame`, `Camera`/`CameraDriver`/`Mount` traits, `Vec2`/`Mat2`, `SimSky` |
| `rustflare-camera` | `simulator`; `playerone` (feature) |
| `rustflare-mount` | `zwo` (serial), `simulator` |
| `rustflare-tracking` | `Tracker` (disc, limb, anchor), `Calibrator`, `Guider` — pure, time-injected |
| `rustflare-capture` | `FrameSink` (RAM buffer + writer thread, SER/PNG, FAT32 rollover), `SerWriter`, `write_png`, flats logic — pure, tested |
| `rustflare-server` | `engine` (state machine), `api` (axum), `preview` (JPEG thread, auto-stretch), `storage` (drive selection and safety), `config` (settings file), `status` |

The engine loop: *apply commands → capture one frame → (at ~10 Hz) statistics, Sun location, tracker, guider/calibrator → preview (throttled) → capture job (the frame is moved into the RAM buffer) → throttled status publish*.
Anything that needs the hardware goes through the engine, so there is exactly one place where camera, mount and jobs interact.
Rationale and alternatives: [ADR 0004](../adr/0004-engine-actor-and-transport.md), [ADR 0005](../adr/0005-hardware-abstraction.md), [ADR 0012](../adr/0012-burst-capture-and-ram-buffer.md).

## Where the Sun is, and who may move the mount

- `sunmodel` (server crate): the one estimate of the Sun's centre, radius and drift. Fed by the limb tracker when tracking, by the detector otherwise; read by the overlay, the coronagraph mask and the guider. It gates out measurements that disagree, predicts through dropouts and follows directly while the mount slews.
- `guiding` (tracking crate): `Calibrator` (median positions, re-issues its move every 2.5 s because the mount ends a guide command after 3 s), `Guider` (windowed average, dead band, gentle chained pulses, drift feed-forward, per-burst judgement in lane recordings), `DriftModel` and `DeadReckoning` (keep the framing while the Sun is hidden).
- Lane recordings (`CaptureLanes`): only the reference lane (shortest non-masked exposure) is analysed; long-exposure frames move the Sun model by their frame-to-frame displacement.
- Automatic meridian flip, pause and resume (`ResumeSpec`): the flip pauses a running recording and restarts what is left afterwards.
- Logs (`events.jsonl`, `mount.jsonl` via `rustflare_mount::trace`, lost frames, session log): see [Debugging and logs](../reference/debugging.md).


## Priorities and CPU placement on the Pi

Recording must never wait for anything else. What is set, and why:

| | Setting | Why |
|---|---|---|
| Recorder (`rustflare.service`) | `Nice=-5`, `CPUWeight=500`, `IOWeight=500` (only effective where the kernel has the matching controller and I/O scheduler), `OOMScoreAdjust=-500` | wins every contention for CPU; the last process the kernel kills if memory runs out |
| Log and state-file writes | a dedicated `logwriter` thread at nice 19 (`asynclog.rs`); the engine thread, which handles every camera frame, never touches the SD card for logging | an SD-card stall in the middle of a log line must not stall frames |
| Stacker (`rustflare-stack.service`) | `Nice=19`, `CPUWeight=10`, `CPUQuota=200%`, `CPUAffinity=2 3`, idle I/O class, `OOMScoreAdjust=500` | cores 2 and 3 only: on the Pi 5 the USB and SD-card interrupts are served by core 0, and the recorder's threads keep 0 and 1 to themselves (they may still run anywhere, the stacker may not touch 0 and 1) |
| Not done | realtime scheduling (`SCHED_FIFO`) of the capture threads; moving interrupts (`/proc/irq/*/smp_affinity`); `isolcpus` | a realtime thread that spins can freeze the whole Pi; the interrupts already sit on core 0 (checked on 2026-11-22: all USB3 and SD-card interrupts on CPU0, `irqbalance` off); reserving cores at boot would need a reboot setting per Pi |

`MemoryMax=2G` on the stacker is only enforced where the kernel has the memory cgroup controller: Raspberry Pi OS does not enable it by default (add `cgroup_enable=memory cgroup_memory=1` to `/boot/firmware/cmdline.txt` and reboot). The stacker's telemetry (`start` event, `cgroup_controllers`) says whether it is there. Until then `OOMScoreAdjust` is the protection.

To see whether a stall of the camera had a cause, the recorder logs a `frame_gap` event (in `events.jsonl`) whenever the gap between frames is more than 8 times the usual (and over 0.25 s): the gap, the usual interval, the capture state, the RAM buffer fill, disk speed, free RAM, the load average, and how many log lines had to be dropped. The stacker's telemetry (`irq_per_core`, per-core busy percentage) shows what ran on which core at the time.
