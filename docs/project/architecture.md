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

