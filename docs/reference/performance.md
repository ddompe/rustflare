# Performance

Numbers measured on a Raspberry Pi 5 (4 cores) with a Player One Apollo-M MINI and a USB 3 stick. Your figures depend on the camera, the USB port and the drive.

## Camera frame rate

| Camera and mode | Frame rate |
|---|---|
| Apollo-M MINI, full frame 1944x1472, 16-bit | 57 fps (about 326 MB/s) |
| Same, 8-bit | 82 fps |
| Same, 50 % frame size (968x736) | 153 fps |
| Same, 25 % frame size (480x368) | 153 fps |
| ASI174MM Mini on a USB 2 port, full frame, 16-bit / 8-bit | 8.7 / 17.3 fps |

The full frame is limited by the USB 3 link. At 50 % and 25 % the rate is limited by the exposure, so a shorter exposure raises it. The ASI174MM figure is the USB 2 limit; a USB 3 port and cable are needed for its higher rates and have not yet been measured. Frame size is set in the [Camera tab](../guide/camera.md#frame-size) (Player One and the simulator; not yet available for ZWO ASI).

## Storage speed

- SD card (Pi 5): about 65 MB/s sustained, about 11 fps of full-frame 16-bit video.
- USB 3 stick: 75 to 90 MB/s sustained.

The RAM buffer is what makes faster bursts possible: frames are captured at camera speed and written in the background. See [Capture formats](capture-formats.md#bursts-and-the-ram-buffer).

## CPU load

At full rate (57 fps, 1944x1472) the whole program uses about a third of one of the four cores with nobody watching, and about 46 % with one phone showing the live view. The preview is only produced while someone is watching, so an unattended recording pays none of its cost. There is plenty of headroom for recording, tracking and guiding at the same time.

Profiling notes for developers: [Performance audit](../project/performance-audit.md).

## Stalls at full rate (issue #86)

On a Pi 5 recording a full frame at 82 fps to a FAT32 USB stick, gaps of 1 to 2.4 s with no frame were seen (2026-11-22, before the frame ring). The likely cause is the kernel collecting gigabytes of written frames in memory and then writing them out in one flood, which starves the camera's USB transfers and the rest of the machine. Since 0.6.1 the SER writer smooths this: every 64 MB it starts writing that chunk to the drive without waiting, waits for the chunk before it and drops that one from the page cache (`sync_file_range` and `posix_fadvise`). This is **not yet measured on the Pi**. To check: record at full rate for a few minutes and look in `logs/<date>/events.jsonl` for `frame_gap` events. `waiting_for_camera_ms` large and `prev_frame_work_ms` small means the camera's USB transfer stalled (bus contention with the stick); a large `prev_frame_work_ms` means the engine itself was slow. If gaps remain, lowering the kernel's dirty limits is the next thing to try on the Pi: `sudo sysctl vm.dirty_background_bytes=67108864 vm.dirty_bytes=268435456`.

A `frame_gap` event also carries `memory`: `dirty_mb` and `writeback_mb` (written frames the kernel has not yet put on the drive), `psi_some_avg10` and `psi_full_avg10` (percent of the last 10 s that tasks spent waiting for memory) and `allocstall` (how often a task had to free memory itself; if it rose during the gap, the stall was direct reclaim). A gap with large `dirty_mb`, a high `psi_full_avg10` and a rising `allocstall` means the page cache full of unwritten frames was competing with the frame buffer for memory.
