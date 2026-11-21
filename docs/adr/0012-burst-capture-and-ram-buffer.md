# 0012. Burst capture through a RAM buffer

- Status: Accepted. Writer, rollover, back-pressure and error behaviour are unit-tested; bursts, schedules and PNG output run end-to-end on the simulators (laptop and Pi). Not yet tried with a real high-frame-rate camera or a slow USB stick.
- Date: 2026-11-21

## Context

Some target cameras are very fast, while storage (USB sticks, the SD card) is slow and bursty. The typical solar use is "100 frames every X seconds": short, fast bursts separated by idle time. Writing each frame synchronously on the capture thread makes the disk the frame-rate limit. Analysis (histogram, Sun location, tracking) also ran on every frame, which is wasteful at hundreds of fps.

## Decision

- **A bounded in-memory queue between capture and a writer thread** (`FrameSink` in `rustflare-capture`). The capture thread moves each frame into the queue (no copy) and goes on; the writer streams to disk. Queue capacity = the **RAM buffer** (default 512 MB, configurable in the UI, capped to 70 % of currently available memory), expressed in frames.
- **Back-pressure, not drops:** when the buffer is full, `push` blocks, so capture slows to disk speed but frames are never silently lost. If the disk fails, the writer keeps draining and discarding so the camera side cannot deadlock, and the error stops the capture.
- **Bursts and schedules:** one capture job type covers a single sequence (one burst) and a schedule (`frames or seconds` per burst, `every N s` start-to-start, `bursts` count or until stopped). The camera loop keeps running between bursts, so live view, tracking and guiding continue. A burst starts on schedule even if the previous one is still being written; if capture cannot keep up, the next burst starts as soon as the previous finishes.
- **Formats:** **SER** (one file per burst) or **PNG images** (one folder per burst, numbered frames). A `.json` sidecar per burst records settings and tracker/mount state.
- **FAT32 rollover:** SER files split into `_part2`, `_part3`, … before the filesystem limit, so long bursts on FAT32 do not fail.
- **Flushing is visible and safe:** after the last burst the state is `flushing` ("writing N buffered frames to disk") and storage/camera changes are blocked until the writer is done; service stop flushes before exiting (`TimeoutStopSec=180`).
- **Per-frame work is minimal:** analysis, statistics and tracking run at ~10 Hz regardless of camera rate; the recording path only moves the frame.

## Alternatives considered

- *Drop frames when the buffer is full*: simpler, but loses data silently; "wait" keeps every frame at the cost of speed. A drop policy could be added as an option for lucky imaging where timing matters more than completeness.
- *`mmap` / `tmpfs` staging*: no better than an explicit queue and less controllable.
- *One writer thread per file*: unnecessary; the disk is the bottleneck.
- *Streaming capture from the SDK (`stream`)*: would raise the achievable frame rate further; the capture call is still one-exposure-at-a-time today (a known limitation, see Roadmap).

## Consequences

- The disk speed shows in the UI (`disk MB/s`), and the Capture panel estimates whether a burst fits the buffer and the average disk load of a schedule.
- Memory use is predictable and bounded by the configured buffer.
- At very high frame rates the per-frame SDK call overhead, not the disk, becomes the limit.
