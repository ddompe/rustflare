# Spike: NEON and the GPU for the stacker's inner loops (2026-11-22)

Question: the stacker takes about 7 s per 100-frame burst on a Pi 5. Is there a cheap speed-up in hand-vectorised code (NEON), a Pi-specific library, or the Pi's GPU?

## Result in one table

| What | Mac (M4) | Pi 5 |
|---|---|---|
| First pass per frame (flat, 2x2 bin, sharpness), the code we had | 3.61 ms | 18.46 ms |
| the same work fused and written for the compiler to vectorise (portable, safe Rust) | 0.73 ms (5.0x) | 3.57 ms (5.2x) |
| explicit NEON intrinsics | 0.34 ms (10.5x) | **2.70 ms (6.8x)** |
| Shifted bilinear accumulate of one frame, the code we had | 3.11 ms | 14.07 ms |
| explicit NEON | 0.51 ms (6.1x) | **3.58 ms (3.9x)** |
| The same first pass as a compute shader (calibrate, bin, Laplacian sums), compute only | 1.69 ms | **17.12 ms** |
| the same including upload and reading the sums back | 3.69 ms | 20.20 ms |
| one CPU core with the NEON kernels (same binary, same machine) | 0.76 ms | **3.08 ms** |

- **NEON is worth it and now in the stacker** (`crates/stack/src/simd.rs`): the calibrate-and-bin kernel, the masked sharpness sums and the shifted accumulate use NEON intrinsics on aarch64 and a portable version elsewhere. A test checks they agree with the portable ones (relative difference below 1e-5), and the real bursts give the same detail measures as before (0.00035 and 0.00094).
- **End to end on the Pi 5** (real field-day bursts, 2 threads): the first pass fell from 1 to 2 s to about 0.7 s, and a burst from **7.2 s to 6.2 s** (4 threads: 5.6 s to 5.0 s). The gain is smaller than the kernels' because the rest of the time is **FFT**: the global alignment (about 2 s) and the per-point correlations (2.4 to 3 s) are dominated by FFTs, which `rustfft` already runs with NEON.
- **The GPU is not worth it.** The Pi's VideoCore VII (V3D 7.1.7, Mesa V3DV, Vulkan 1.3) works through `wgpu` and the shader gives the same numbers, but even with no upload and no readback it needs 17 ms per frame against 3.1 ms for **one** CPU core. Small, memory-bound kernels on a small GPU that shares RAM with the CPU gain nothing; on the Mac's much stronger GPU the CPU still wins. A GPU FFT could be a different story but would be a large piece of work for a stage that is already about 2 s.
- The spike code is `spikes/gpu-binlap` (outside the workspace) and `crates/stack/examples/simd_spike.rs`; both print the table above.

## Libraries (open source, ARM-optimised): what exists

Checked by search on 2026-11-22; "unverified" means I know of it but did not confirm it today.

| Library | What it offers | Fit here |
|---|---|---|
| **libyuv** (BSD-3, Google) | hand-written AArch64 NEON for plane scaling, including 16-bit box and linear 2x downscale (`ScaleRowDown2Box_16_NEON`, per its unit tests) | exactly our 2x2 bin; C++ (FFI); we now have the same in Rust |
| **KleidiCV** (Arm, Apache-2.0, gitlab.arm.com/kleidi/kleidicv) | AArch64-tuned image resize, Gaussian blur, Sobel, erode/dilate, colour conversions; picks the fastest code path for the CPU; plugs into OpenCV | useful for a later debayer, blur or resize; C++; the search did not say NEON versus SVE |
| **pic-scale** (Rust crate) | image scaling with NEON kernels for 16-bit planes | a pure-Rust option if we need general resizing |
| **rustfft 6.4.1** | what we use; NEON kernels on aarch64 | already in |
| **oxifft** (Rust), **pffft** (C, NEON, sizes multiple of 16), **FFTW** (NEON), **Ne10** (Arm's older NEON library with FFT) | FFT alternatives | the next thing to try: our remaining time is FFT; none was benchmarked here against `rustfft` at our sizes (480x360 and 80x80) |
| Arm Compute Library, XNNPACK | NEON/assembly kernels, aimed at neural networks | unverified; not a fit |

Most of these are C or C++ with NEON *intrinsics*; hand-written assembly is rare (libyuv has some). Writing intrinsics in Rust, as we did, is the same technique without a C++ dependency.

## FFT shoot-out (same day, afterwards)

What was left after NEON was FFT, so the same questions for the two FFT sizes the stacker uses: the global alignment (480x360 on 4x4-binned frames) and the per-point correlations (80x80). One 2D forward transform, in milliseconds (spike: `spikes/fft-shootout`; PFFFT's C source comes from the `pffft_rust` crate and is not committed):

| | 480x360 Mac | 480x360 **Pi 5** | 80x80 Mac | 80x80 **Pi 5** |
|---|---|---|---|---|
| A `rustfft`, complex 2D, plain transpose (what we had) | 1.00 | 3.74 | 0.035 | 0.098 |
| B the same with a tiled (cache-friendly) transpose | 0.92 | 2.88 (1.30x) | 0.033 | 0.095 (1.03x) |
| **C `realfft`: real-to-complex rows, half the spectrum** | 0.51 | **1.52 (2.46x)** | 0.017 | **0.052 (1.87x)** |
| D PFFFT (C, NEON), complex | 0.58 (480x384) | 2.53 (1.48x) | 0.018 | 0.055 (1.77x) |

- **`realfft` wins, in pure Rust**: an image is real, so half the spectrum is redundant. It beats even the C library's complex transform, so there is no reason to add a C dependency. (PFFFT's real transform, `oxifft` and FFTW's NEON build were not tried: FFTW needs a system library to cross-compile, and the gain over `realfft` would have to be large to justify either.)
- **Adopted** (`crates/stack/src/fft2.rs`, used by both the global and the per-point aligner). Results are unchanged on the real bursts (detail measures 0.00035 and 0.00094) and in the tests.
- The scoring pass (a Laplacian and a masked covariance per frame) got the same vectorised treatment (`simd.rs`).

**End to end on the Pi 5** (the same real bursts, 100 frames of 1944x1472, warm cache):

| | at the start (scalar, complex FFT) | NEON kernels | + `realfft` and vectorised scoring |
|---|---|---|---|
| disc burst, 2 threads | 7.2 s | 6.2 s | **5.0 s** |
| disc burst, 4 threads | 5.6 s | 5.0 s | **4.4 s** |
| prominence burst, 2 threads | 7.2 s | 6.8 s | **4.4 s** |

(Read from the stick cold, which is what the service does now that it drops each recording's cache when it is done, the first phase takes about 1.5 s longer.) The service's own timing next to a recording is in the telemetry (about 8 to 9 s per burst under the service's CPU quota, with a writer hammering the same stick).

## Next steps, if the time matters

1. Done: the scoring pass and the FFTs.
2. What is left is spread thin: the cold read of the recording (1.5 s from this stick), the per-point window extraction and the window/high-pass steps before each FFT. Another 10 to 20 % is possible with effort; the stage is no longer the bottleneck next to the camera and the stick.
3. Leave the GPU.
