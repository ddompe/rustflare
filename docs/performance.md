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

## Not done yet (issue #67)

- A Pi 5 specific build (`-C target-cpu=cortex-a76`): safe only if Pi 4 and older are not supported; keep the generic build as the default and offer a Pi 5 flavour if profiling on the Pi shows a win.
- `Arc<Frame>` sharing, reusing the JPEG and gray buffers, and moving the tracker's per-frame passes to strided sampling.
- A profile with recording on and several viewers.
