# 0008. Tracking modes, automatic calibration, pulse guiding

- Status: Accepted
- Date: 2026-11-21

## Context

The Sun drifts out of the field in minutes. Vidoxide offers two trackers (centroid of a user rectangle, and an
"anchor" block-matching tracker) and a guiding loop; its calibration is manual (the user slews each axis and marks
the direction). For solar work the target size relative to the field varies a lot: a short focal length shows the whole disc,
a long one shows a small region, often with a sunspot of interest.

## Decision

Three tracking modes behind one `Tracker` type, selectable in the UI:

1. **Full disc** — threshold at half the peak, centroid and equivalent radius of the bright region (subsampled ×4). Needs the whole disc in frame.
2. **Partial disc (limb)** — *new*. Finds limb edge points (excluding edges that are merely the frame border), fits a circle (Kåsa algebraic fit; radius is fitted once from the first frame, or given by the user, then locked and only the centre is refined by Gauss–Newton). Works when part of the disc is out of frame, provided a decent arc of limb is visible.
3. **Feature (anchor)** — ported from Vidoxide: a 64×64 reference block is matched by coarse-to-fine sum-of-squared-differences within ±20 px. Works on any structured region (sunspot, plage, prominence).

All modes reject implausible jumps (a frame whose measured position moves by more than half a disc radius is held at the previous position), and report `lost` instead of failing the loop.

**Calibration is automatic** (`Calibrator`): with sky tracking on, pulse-guide the primary axis for 4 s, let it settle, measure the image displacement; repeat for the secondary axis; build the image→axes matrix (rejecting near-parallel axes or insufficient motion). This replaces manual marking.

**Guiding** (`Guider`) is Vidoxide's scheme: if the tracked position is more than a margin from the target, command a guide speed along the matrix-transformed error direction for ~1 s, re-evaluate; otherwise stop and re-check every 2 s.

All three are pure state machines with injected time, tested in closed loop against a simple physical model (`guiding::tests`).

## Consequences

- The anchor tracker keeps its reference block fixed; slow evolution of the feature (seeing, rotation) will eventually degrade matching — re-pick if it drifts.
- Anchor matching assumes spatially correlated structure; it will not lock on pure noise (verified in tests).
- Limb mode accuracy depends on arc length; the fitted radius is only as good as the first frame. Supplying the radius (from a full-disc frame or the optics) is more robust.
- No tracker is validated on real solar data yet — only synthetic frames and the simulator.
