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

## Next steps, if the time matters

1. The scoring pass after the global alignment still uses scalar loops (a Laplacian, a masked covariance): the same treatment should save roughly another second per burst.
2. An FFT shoot-out at our two sizes (`rustfft` against `pffft` and `FFTW` NEON); the global alignment could also use fewer, smaller transforms.
3. Leave the GPU.
