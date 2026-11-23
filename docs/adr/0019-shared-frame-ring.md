# 0019. One shared frame ring for capture, raw writing and stacking

- Status: Proposed (design 2026-11-22, building in stages)
- Date: 2026-11-22

## Context

Frames currently go camera thread, bounded in-memory queue, writer thread, SER file; the separate stacker then reads the files back. At 100 frames every 30 s that is 68 GB an hour of video that is deleted after stacking, a cold re-read from the stick for every burst, and the stacker competing with the writer for the stick (-16 % write speed measured). The owner wants to stack from memory, keep only the stacked images in the common case, keep raw for events like eclipses, and not give up RAM that could buffer longer bursts.

Modes wanted: (1) raw only, (2) raw and stack, (3) stack only, (4) only the best N % of frames as raw; the percentages configurable.

## Decision

**One single-producer, multi-consumer ring of frame slots in shared memory** replaces the queue.

- **Memory:** a `memfd` (a plain file on macOS for development) of the whole RAM buffer, mapped by the recorder (read-write) and the stacker (read-only). The descriptor is handed over a Unix socket; either process can give it back to the other after a restart; it disappears when the last holder exits. Fixed geometry per ring (frame size, depth); a change of camera mode makes a new ring.
- **Layout:** a control area (header, consumer table, burst table) followed by slots; a slot is a 64-byte header (commit sequence, timestamp, burst id, exposure, gain, flags) and the frame bytes, 64-byte aligned for NEON. The camera side writes a frame into the next slot and publishes its sequence number last (release); a frame exists only when committed, so a crash mid-frame leaves nothing half-visible.
- **Capture never waits for a consumer.** The producer wraps around; a consumer that is slow, stalled or dead is the one that has to catch up, and loses what was overwritten. Consumers attach to the header with a name, a pid, a heartbeat and a read cursor. A *best-effort* consumer (the stacker, preview, debugging taps, and the raw writer in the "drop frames" mode) can be lapped and resynchronises or discards the work it was doing. The one exception, which keeps today's "wait for the disk" mode, is a consumer that asks to be *must-not-lose*: only the raw writer in that mode, and it holds the producer back exactly as the bounded queue does now. A consumer can **pin** a sequence range (a burst) to read it in place, in any order, any number of times, with zero copies; for a best-effort consumer the pin is advisory (it shows what it is working on) and the consumer checks that each frame is still intact.
- **Bursts** are described in a table in the shared memory (id, first sequence, count, mode, percentages, name, sidecar JSON, the stacker's result: which frames it kept). Everything needed to resume after a crash is in the shared memory, not in a process.
- **Death of a process:** the recorder dying leaves the committed frames in the ring; a new recorder re-attaches and carries on numbering, and the stacker (still holding its mapping) can finish the burst it was on; a burst left open is closed as partial. The stacker dying holds nothing up (it was never allowed to); its stale heartbeat and pid let the recorder see that the stack did not happen, and the burst is then written raw if its frames are still in the ring (the gap check below makes that the normal case). Only a reboot, or both processes dying at once, loses frames that were not on disk yet.
- **Modes become consumers of a burst:** 1 raw writer only; 2 raw writer and stacker; 3 stacker only (raw written only if the stack failed); 4 the stacker ranks the frames first and the raw writer then writes only the best `raw_keep_percent` of them. The stack uses the best `stack_keep_percent`. A burst that the gap check says the stacker cannot finish before its frames are overwritten falls back to mode 1. The check is a **wrap budget**: the frames that can still be committed before the burst's first frame is overwritten (`frames_until_overwritten`) divided by the frame rate, against the measured stack time with a margin. For the usual plan (100 frames, then quiet) the ring never wraps over a burst and the check passes easily; a continuous full-rate recording fails it and is written raw, which is right.
- **Never block the camera for a stacker.** There is no hold to time out: if a stack job finds its frames overwritten it discards the work and the burst falls back to raw if the frames are still there.

## Alternatives considered

- *Copy bursts to the stacker over a socket:* simplest, keeps isolation, but doubles the memory for a burst and costs a copy of 570 MB; rejected because the owner does not want to waste buffer memory.
- *Stack inside the recorder process:* zero-copy and simple, but a crash or runaway memory in the stacker takes the capture down and the CPU, memory and affinity limits set per service no longer apply.
- *A ring where a slow consumer holds the producer back (head-of-line blocking), with a timeout:* first drafted; rejected on the owner's suggestion that the writer should simply wrap and the slow or dead consumer should be the one to catch up. It keeps the camera path free of any consumer, removes the need to reap dead consumers to unblock capture, and the wrap budget protects a burst in the cases that matter.
- *A slab/free-list allocator instead of a ring:* avoids an old burst being overwritten while still needed, at the cost of fragmentation handling and a free-list protocol shared between processes; the wrap budget and the large headroom (at the camera's full rate 4 GB lasts about 12 s, a stack takes 5 to 10 s) make it unnecessary.
- *A named file in `/dev/shm`:* any process can attach by name without descriptor passing, but Debian's default cap is 50 % of RAM (exactly the 4 GB), a stale file leaks until reboot, and the size cannot change at run time. memfd avoids all three.

## Visibility of errors

Every failure of the pipeline (a burst falling back to raw, a stack that failed or found its frames overwritten, dropped frames, a consumer or the producer dying or being lapped, a writer error, a refused mode) is shown on the page until acknowledged, written as an event, listed in `status.errors`, and flagged on the burst and in the stack's metadata. Nothing is quiet. The margin of the stack-time check (default 2x) is a setting, not a constant. Details: `docs/project/design-recording-modes.md`.

## Consequences

- The raw writer becomes a consumer thread reading slots in place; the queue, its per-frame allocation and the stacker's re-read of the file go away.
- Stack-only recording cuts disk use from about 68 GB/h to about 1.3 GB/h (float FITS) at the usual cadence, and takes the stacker off the stick.
- A new crate `rustflare-ring` holds the unsafe core (mapping, atomics) behind a safe API with multi-thread, multi-mapping and crash-recovery tests.
- Stacking settings and algorithm are fixed for frames that are not kept (modes 3 and 4): re-stacking later needs raw.

## Plan

1. Done: `rustflare-ring`: ring, producer, consumers, pins, heartbeats, crash recovery, tests.
2. Done: the stacker reads frames through a `FrameSource` (a SER file, frames in memory, or a burst in the ring); a stack from memory or from the ring is bit-identical to one from the file (single thread), and a frame that cannot be read fails the job with its reason instead of being skipped.
3. The raw writer as a ring consumer replacing `FrameSink`'s queue; policies and counters preserved.
4. Burst table, the stacker's `serve` mode, the descriptor handover and reconnect, the gap check and the raw fallback.
5. Modes and percentages in the API, the Record tab, telemetry and docs; tests on the simulators and on the real camera.
