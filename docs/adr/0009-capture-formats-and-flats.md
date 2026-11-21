# 0009. SER for sequences, PNG for stills, automatic flats

- Status: Accepted
- Date: 2026-11-21

## Context

Requirements: capture single frames or sequences, and flats.

## Decision

- **Sequences → SER** (de-facto standard for AutoStakkert!, PIPP, Siril, etc.), uncompressed, 8 or 16-bit mono, with the per-frame **timestamp trailer** (Vidoxide left that as a TODO). Header signature is `LUCAM-RECORDER`. The endianness flag follows Vidoxide's note that real-world files use `0` for little-endian data, contrary to the published spec.
- **Single frames → PNG** (8/16-bit grayscale; 16-bit samples are byte-swapped to PNG's big-endian).
- **Flats** are a capture mode of their own: the engine measures the mean level of each frame and adjusts exposure (assuming a linear sensor; clipped frames halve the exposure) until the mean is within ±5 % of a target (default 50 % of full scale). It then records N frames to a SER file **and** averages them into a 16-bit `*_master.png`. The previous exposure is restored afterwards. Tracking and guiding are stopped (flats are taken on a diffuser or defocused sky). It gives up with a clear message when the target is unreachable at the exposure limits.
- Every capture writes a `.json` **sidecar** with camera controls, mount state and tracker state, so a flat can be matched with its lights.
- Files go to `<data-dir>/<UTC date>/<kind>_<UTC time>[_name].*`.

## Alternatives considered

- *FITS* is the astronomy-standard for single frames and masters; not needed by the planetary/solar stacking tools used here. Planned as an optional output.
- *Dark frames.* Out of scope for the first iteration (the same job framework can host them).

## Consequences

Large files: a 1280×960 16-bit sequence is ~2.4 MB/frame; plan the Pi's storage accordingly. The SER header's local-time field is written as UTC (the Pi's timezone is not assumed).
