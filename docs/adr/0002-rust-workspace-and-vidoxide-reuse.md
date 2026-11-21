# 0002. Rust workspace; port (not fork) Vidoxide code

- Status: Accepted
- Date: 2026-11-21

## Context

[Vidoxide](https://github.com/GreatAttractor/vidoxide) (MIT) is a Rust desktop capture
application with solar-relevant features: image tracking, mount guiding, a ZWO mount driver,
SER output. It is built on GTK and its own `ga_image` crate, is single-process and
callback/timer driven on the GTK main loop, and has no Player One support. We want a headless,
web-controlled application.

## Decision

RustFlare is a new Cargo workspace. We **port** the useful algorithms and protocol code onto our
own small types instead of forking the application:

| Crate | Responsibility | Taken from Vidoxide |
|-------|----------------|---------------------|
| `rustflare-core` | `Frame`, `Camera`/`Mount` traits, geometry, simulator shared state | mount speed types |
| `rustflare-camera` | drivers (Player One, simulator) | design of the `Camera`/`Driver` traits |
| `rustflare-mount` | drivers (ZWO, simulator) | `mount/zwo.rs` |
| `rustflare-tracking` | trackers, calibration, guiding | `tracking.rs` (anchor), `guiding.rs` |
| `rustflare-capture` | SER/PNG writers, flats logic | `output/ser.rs` |
| `rustflare-server` | engine + HTTP/WS API | — |

Vidoxide is MIT-licensed; RustFlare is MIT-licensed too, keeps the upstream licence text
(`LICENSE.vidoxide`) and lists every ported piece in `NOTICE.md`.

## Alternatives considered

- *Fork Vidoxide and swap the GUI.* Its GUI, timers and state (`Rc<RefCell<ProgramData>>`) are intertwined with GTK; a headless server would need most of it rewritten anyway.
- *Depend on Vidoxide as a library.* It is a binary crate without a stable library API.
- *Keep `ga_image` and `cgmath`.* Large dependencies (and native image libraries) for what is a few hundred lines of mono-frame logic here.

## Consequences

We own and test the ported code (unit tests were added where Vidoxide had few). Upstream fixes
must be merged by hand; `NOTICE.md` records the upstream commit-level origin of each file.
Only mono 8/16-bit frames are supported for now (see ADR 0009 for colour as future work).
