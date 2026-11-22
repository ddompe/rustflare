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
