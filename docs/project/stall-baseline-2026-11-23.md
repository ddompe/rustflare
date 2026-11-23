# Capture stall baseline on the Pi (2026-11-23)

What the recorder and the Pi do during a full-rate recording to the FAT32 USB stick, measured before deciding on direct (`O_DIRECT`) writes ([#86](https://github.com/ddompe/rustflare/issues/86)). Method: `scripts/pi-io-baseline.py` on the Pi, a 60 s sequence with the drop policy and a 6 GB buffer, the live view open (MJPEG stream), no mount and no guiding. Camera 1944x1472, 16-bit, 57 fps, which is 328 MB/s into a stick that takes about 80 to 100 MB/s. Two builds: the released 0.6.0 (page cache, no write-behind) and 0.6.1 (the SER writer starts writeback every 64 MB and drops the chunk before from the cache).

| | 0.6.0 | 0.6.1 |
|---|---|---|
| Camera fps (min / mean) | 56.6 / 57.9 | 48.3 / 60.3 |
| Frames on the drive after 60 s of recording | 1829 | 1500 |
| Frames dropped (buffer full, expected) | 1609 | 1869 |
| `frame_gap` events (gaps over 0.25 s) | 1 (0.53 s) | 0 |
| Dirty page cache, peak | 857 MB | 265 MB |
| Direct reclaim (`allocstall`, `pgscan_direct`) | 0 | 0 |
| Drive writes (mean / peak MB/s) | 81 / 109 | 58 / 111 |
| Seconds of 258 / 299 with under 10 MB/s to the drive | 12 | 26 |
| I/O wait (mean / peak %) | 37 / 59 | 35 / 78 |
| Threads, mean / peak % of a core | preview 23 / 30, engine 14 / 48, frame-writer 13 / 100 | preview 24 / 38, engine 20 / 98, frame-writer 19 / 100 |
| Throttling, peak temperature | none, 64 °C | none, 69 °C |

## What it says
- **The stick is the bottleneck.** It absorbs 60 to 110 MB/s against 328 MB/s coming in, so the buffer fills in about 20 s and most frames are dropped by design. That is the buffer's job, not a stall.
- **No direct reclaim in either build.** The kernel never had to free memory for a recording thread, so the page cache did not starve the frame buffer at this rate. The theory that dirty pages stall the engine is not supported here.
- **Write-behind keeps dirty memory low (857 to 265 MB) but did not clearly help throughput.** 0.6.1 wrote fewer frames in this run (1500 against 1829) and its drive was idle for more seconds. Single runs of the same build vary widely (0.6.1 runs on the same day wrote 1500 to 1755 frames), so this is not a measured loss; it is a reason to repeat before concluding anything. The 64 MB waits are the thing to look at if 0.6.1 turns out slower.
- **One 0.5 s gap in 0.6.0, none in 0.6.1.** Neither is the 1 to 2.4 s stalls seen on 2026-11-22. That recording ran at about 82 fps with guiding and the mount connected, which this one did not.
- **Not reproduced yet:** the 1 to 2.4 s stalls. The next attempt needs the mount, guiding and the 82 fps mode (8-bit or a smaller frame). The sampler and the `frame_gap` event (now with kernel memory counters and `waiting_for_camera_ms`) will say which part stalled.

## Direct writes
Direct writes exist behind a switch (More > Storage). In two runs of each mode (40 s with a 4 GB buffer, 60 s with 6 GB) the files were valid SER files with the same number of frames and the same stalls (none over 0.3 s); the drive's true speed shows as 76 to 99 MB/s instead of the page cache's 700 to 860 MB/s. Nothing so far argues for making it the default.
