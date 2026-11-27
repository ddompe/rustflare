# Architecture

```
 Browser (React) ──REST──► axum handlers ──Command──►  ENGINE THREAD
     ▲  ▲                                                │ owns:
     │  └── WebSocket: Status (watch channel) ◄───────────┤  Camera (blocking capture)
     └───── MJPEG /stream.mjpg ◄── preview thread ◄──────┤  Mount
                          (1-slot, drops frames)         │  Tracker → Guider/Calibrator
                                                         │  Capture job (single / bursts / flats)
                                                         │      └─► frame ring (shared memory) ─┬─► writer thread ─► files
                                                         │                                      └─► stacker (other process, via stack.sock)
                                                         └─  Storage (chosen drive, free space, safety)
```

Crates:

| Crate | Role |
|---|---|
| `rustflare-core` | `Frame`, `Camera`/`CameraDriver`/`Mount` traits, `Vec2`/`Mat2`, `SimSky` |
| `rustflare-camera` | `simulator` (mono, colour, cooled colour; scenes for the Sun, a flat, the Moon and a planet); `playerone` (feature); `asi` (feature, loads `libASICamera2` at run time) |
| `rustflare-mount` | `zwo` (serial and Bluetooth, feature `ble`), `ioptron` (serial), `simulator`; `detect` asks a serial port which of them answers |
| `rustflare-tracking` | `Tracker` (disc, limb, anchor), `Calibrator`, `Guider` — pure, time-injected |
| `rustflare-ring` | the shared frame ring: one producer, any number of consumers, in memory that another process can map (ADR 0019); `ipc` passes its descriptor over a unix socket |
| `rustflare-capture` | `FrameSink` (the frame ring and a writer thread that is one consumer of it: SER/PNG/FITS, FAT32 rollover, wait or drop when full, deferred bursts), `SerWriter`, `write_png`, flats logic — pure, tested |
| `rustflare-stack` | the stacker; `serve` takes bursts from the recorder's ring over `stack.sock`, `watch` stacks finished recordings from disk |
| `rustflare-server` | `engine` (state machine; after each burst it hands the burst to the stacker through `stackbridge` and decides what is written), `api` (axum), `stack_api` and `browse` (stacker settings, the file manager's listing and tar download), `preview` (JPEG thread, auto-stretch), `storage` (drive selection and safety, the internal-disk reserve), `setups` (named optical setups), `sitetime` (the mount's own site and clock against the Pi's), `cooler` (the gentle warm-up), `config` (settings file), `status` |

The engine loop: *apply commands → capture one frame → (at ~10 Hz) statistics, Sun location, tracker, guider/calibrator → preview (throttled) → capture job (the frame is moved into the RAM buffer) → throttled status publish*.
Anything that needs the hardware goes through the engine, so there is exactly one place where camera, mount and jobs interact.
Rationale and alternatives: [ADR 0004](../adr/0004-engine-actor-and-transport.md), [ADR 0005](../adr/0005-hardware-abstraction.md), [ADR 0012](../adr/0012-burst-capture-and-ram-buffer.md).

## Where the Sun is, and who may move the mount

- `sunmodel` (server crate): the one estimate of the target's centre, radius and drift (named for the Sun; for the Moon and a planet the same model is fed by their own measurer, see below). Fed by the limb tracker when tracking, by the detector otherwise; read by the overlay, the coronagraph mask and the guider. It gates out measurements that disagree, predicts through dropouts and follows directly while the mount slews.
- `guiding` (tracking crate): `Calibrator` (median positions, re-issues its move every 2.5 s because the mount ends a guide command after 3 s), `Guider` (windowed average, dead band, gentle chained pulses, drift feed-forward, per-burst judgement in lane recordings), `DriftModel` and `DeadReckoning` (keep the framing while the Sun is hidden).
- **Profiles and the target** ([ADR 0021](../adr/0021-use-profiles.md)): `status.profile` (`sun_white`, `sun_ha`, `moon`, `planet`, `other`) is kept by the engine, because the engine needs it. It chooses the measurer that feeds the one model (the disc and limb detector for the Sun and Moon, `locate_planet` for a planet, none for `other`), the expected radius (`status.optics.target_radius_arcsec`), the target of the meridian flip (`status.flip_target`, with its own tracking rate), the FITS `OBJECT` and the clipping rule. `status.target` says what the profile images. The API refuses nothing because of the profile; the UI greys controls with a reason.
- **Optical setups** (`setups.rs`): named trains saved in the settings file, applied through the same code as `PUT /api/optics` and `PUT /api/profile`; `status.optics.setup` says which is in use and whether it was modified.
- **The mount's own site and time** (`sitetime.rs`): the driver reads what the mount holds (`Mount::read_site_time`), the engine compares it with the Pi's every 30 s (`status.mount.site_time`) and can send the Pi's, by request or once after connecting ([ADR 0023](../adr/0023-mount-site-and-time-sync.md)). A mount without a battery (the HAE69C) restarts from 2017-01-01.
- **Mount drivers** report what the UI needs (`slew_speeds`, `can_search_home`, `link`, `site_time`): the UI builds its controls from them, so a new mount needs no UI change.
- **The screen** is fixed-size with overlay chips ([ADR 0022](../adr/0022-fixed-view-and-overlay-chips.md)); the status carries what it needs (`version`, `notices`, `capture`, `after_burst`).
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

`MemoryMax=2G` on the stacker is only enforced where the kernel has the memory cgroup controller: Raspberry Pi OS does not enable it by default (the firmware adds `cgroup_disable=memory`). `sudo rustflare-memcg` (scripts/pi-enable-memory-cgroup.sh) adds `cgroup_enable=memory cgroup_memory=1` to `/boot/firmware/cmdline.txt` (keeping a backup) and a reboot applies it; it is done on the development Pi, verified on 2026-11-22. The limit counts the page cache too, so the stacker drops each recording's cached pages when it has finished with it. The stacker's telemetry (`start` event, `cgroup_controllers`) says whether it is there. Until then `OOMScoreAdjust` is the protection.

To see whether a stall of the camera had a cause, the recorder logs a `frame_gap` event (in `events.jsonl`) whenever the gap between frames is more than 8 times the usual (and over 0.25 s): the gap, the usual interval, how long the engine spent on the previous frame (`prev_frame_work_ms`) and how much of the gap was spent waiting for the camera (`waiting_for_camera_ms`), the capture state, the RAM buffer fill, disk speed, free RAM, the load average, and how many log lines had to be dropped. The stacker's telemetry (`irq_per_core`, per-core busy percentage) shows what ran on which core at the time.
