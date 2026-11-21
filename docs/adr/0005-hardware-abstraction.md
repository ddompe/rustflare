# 0005. `Camera` / `Mount` traits, drivers, simulators

- Status: Accepted
- Date: 2026-11-21

## Context

Today's target is one camera (Player One) and one mount family (ZWO AM3/AM5). More will follow.

## Decision

- `Camera` (open device) and `CameraDriver` (enumerate/open) are traits in `rustflare-core`. Camera settings are exposed as a **list of `ControlDesc`** (`id`, range, step, value, unit, log-scale hint) rather than a fixed struct, so the UI renders whatever a driver offers. A few ids are standardised (`exposure_us`, `gain`, `offset`, `high_bit_depth`) because the engine needs them (flats).
- `Mount` is a trait: `set_tracking`, `set_tracking_rate`, `guide`, `slew`, `stop`. Speeds are `RadPerSec`, as in Vidoxide.
- Drivers are registered in one place (`rustflare_camera::drivers`, `rustflare_mount::connect`); hardware drivers sit behind Cargo features where they have native dependencies (`playerone`).
- **Simulators** are first-class: a simulated camera renders a limb-darkened Sun (or a vignetted flat field) displaced by a `SimSky` that the simulated mount moves. This makes calibration, guiding, recording and flats testable with no hardware, in CI, and for UI development.

## Consequences

Adding a camera or mount means one new file and one registration line. The trait is deliberately small and
blocking; features like cooling, ROI, binning or colour will need new optional methods or control ids (see Roadmap).
