# Performance notes

Measured on the target hardware (Raspberry Pi 5, 4 cores, 2026-11-21): Player One Apollo-M MINI, 1944x1472 RAW16 at ~57 fps (~326 MB/s), one viewer on the phone, no recording.

| Thread | CPU (share of one core) | What it does |
|---|---|---|
| `preview` | ~26 % | downscale to <=1280 wide, optional flat-field correction, stretch, JPEG (jpeg-encoder, quality 75) at <=15 fps |
| `engine` | ~18 % | frame intake, mean/histogram, Sun detection, status |
| `engine` (second) | ~15 % | tracker / guider work |

Total about 60 % of one core out of four: plenty of headroom while recording (the SER write path is I/O bound: the USB 3 stick writes 75-90 MB/s sustained, see the USB 3 check in the hardware checklist).

## What was checked

- **Flat-field correction** is a Q12 fixed-point multiply per pixel (`FlatField::apply`), a branch-free loop that the compiler vectorises (NEON on aarch64). It runs only on the preview copy, so recordings stay raw and the cost is limited to the preview frame rate.
- **Preview conversion** now uses integer arithmetic only (16.16 multiplier, direct row slicing). On a desktop benchmark the conversion loop was not the bottleneck: JPEG encoding of the 972x736 picture dominates (about 3 ms on an Apple-silicon Mac). So the cheapest further saving is less JPEG work, not faster pixel loops.
- The engine sends the preview thread a **clone of the whole frame** (5.7 MB) about 15 times a second. Sharing the frame buffer (`Arc<Frame>`) would remove that copy; planned.

- **Preview only while someone watches.** The engine counts live viewers (the MJPEG stream) and `/snapshot.jpg` requests; with nobody watching it skips the frame copy, the downscale and the JPEG encode. An unattended recording therefore pays none of the preview cost. On the simulator the process went from 15 % to 11 % of a core; the Pi figure is in the table above for the viewer case.

- **Buffer reuse**: the server sets glibc's `M_MMAP_THRESHOLD` to 128 MB, so multi-megabyte frame buffers stay on the heap and are reused instead of being mmapped, page-faulted and unmapped for every frame. A Pi measurement before and after is on the hardware checklist (close the phone tab first).

## Not done yet (issue #67)

- A Pi 5 specific build (`-C target-cpu=cortex-a76`): safe only if Pi 4 and older are not supported; keep the generic build as the default and offer a Pi 5 flavour if profiling on the Pi shows a win.
- Reusing the JPEG and gray buffers, and moving the tracker's per-frame passes to strided sampling.
- A profile with recording on and several viewers.

## Region of interest (measured on the Pi 5 with the Apollo-M MINI, 2026-11-21)

| Frame size | Camera fps |
|---|---|
| Full, 1944x1472 | 57 |
| 50 %, 968x736 | 153 |
| 25 %, 480x368 | 153 |

At 50 % the frame rate is limited by the exposure (6.46 ms at the current setting, about 155 fps), not by USB: the full frame is limited by the USB 3 readout (~326 MB/s). A shorter exposure would raise the small-frame rate further. ROI is centred (Camera > Frame size); Player One and the simulator support it, ZWO ASI is not done yet (the driver needs `ASISetStartPos`).
