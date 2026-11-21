# 0004. Single engine thread, REST + WebSocket + MJPEG

- Status: Accepted
- Date: 2026-11-21

## Context

Camera SDKs and serial mounts are blocking. Capture, tracking, guiding and recording all
consume the same frames and must agree on state (e.g. flats stop the tracker; guiding
needs the latest tracker position). Vidoxide solved this with a GTK main loop and shared
`Rc<RefCell<>>` state.

## Decision

One dedicated **engine thread** owns the camera, mount, tracker, guider and current capture job.
Its loop is: apply pending commands, grab one frame, run tracker → guider → capture job, publish.
HTTP handlers never touch hardware; they send a `Command` over a channel and await a one-shot reply.

Outbound data:

- **State** — a `Status` snapshot in a `tokio::sync::watch`, pushed to browsers over WebSocket (`/api/ws`, ≤ ~7 Hz) and available at `GET /api/status`.
- **Preview** — JPEG encoded on a *separate* thread that is fed frames with `try_send` on a 1-slot channel, so slow encoding drops preview frames instead of delaying capture. Served as `multipart/x-mixed-replace` MJPEG (`/stream.mjpg`), which works in a plain `<img>`.
- **Files** — written by the engine, served read-only from the data directory.

## Alternatives considered

- *Locks around each device in handlers.* Races between guiding, calibration and capture jobs; hard to reason about.
- *WebRTC/H.264 preview.* Better bandwidth, far more complexity; MJPEG at 1280 px is adequate on a LAN.
- *Binary WebSocket frames.* Possible later; MJPEG needs no client code.

## Consequences

Simple, deterministic state machine; all timing-sensitive logic is single-threaded.
Commands are processed between frames, so a very long exposure delays command handling by up to one exposure.
There is **no authentication**: the server controls a physical mount and must only be exposed on a trusted network (see Getting started).
