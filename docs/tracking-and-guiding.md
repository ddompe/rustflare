# Tracking & guiding

Decisions and rationale: [ADR 0008](adr/0008-solar-tracking-and-guiding.md).

## Tracking modes

| Mode | Use when | How it works | Limits |
|---|---|---|---|
| **Disc edge (limb)** — default | whole or partial disc, some limb in view | fits a circle to the visible limb; the radius is locked (from you, the Optics setting, or the first fit) and only the centre is refined | needs a reasonable arc of limb; a radius guessed from a short arc is imprecise, so set Optics |
| **Feature (anchor)** | field entirely on the disc, or following a sunspot/filament/plage | 64×64 block matching within ±20 px, coarse-to-fine | needs structure with some spatial correlation; the reference block is not refreshed |
| **Full disc (centroid)** | whole disc in frame | centroid of the bright region, plus disc radius | the whole disc must be visible, else the centroid is biased |

**Finding the disc.** Sky and disc are separated with an Otsu threshold on a log-intensity histogram, which copes with H-alpha images where bright plage and prominences raise the peak and dark filaments lie inside the disc. If the frame does not show two clearly different populations (e.g. a field wholly on the disc) RustFlare says there is no limb in view. See [ADR 0013](adr/0013-partial-disc-first-tracking-and-optics.md).

**Optics.** Focal length (with an optional Barlow/reducer factor, so you do not have to work out the effective focal length yourself) and pixel size give the image scale and, with the Sun's apparent size today (≈ 943–976″), the expected radius in pixels, which makes partial-disc fits much more reliable. A measured radius that disagrees by more than 10 % is flagged (wrong focal length, reducer, binning).

Frames where the measurement jumps by more than half a disc radius are treated as artefacts and ignored. If the target disappears the tracker reports *lost* and keeps trying.

## Calibration

**Yes: the mount's axes have to be mapped to the image once.** Which way "RA+" moves the image depends on how the camera is rotated on the scope, so RustFlare measures it instead of assuming. With solar tracking on, RustFlare guides the primary axis at 0.5× sidereal for 4 s, waits 1 s, and measures how the tracked position moved; then the same for the secondary axis. The two unit vectors give the image→axes matrix. It fails with a message if the image moves less than 5 px, or if the axes come out nearly parallel. The result is shown in the Tracking panel (the image direction of RA+ and Dec+) so you can sanity-check it.

**When to recalibrate.** The mapping is fixed while the camera orientation and the mount's configuration stay the same. Redo it after rotating the camera or diagonal, after a **meridian flip** (the directions reverse), and, if the AM3 is used in **alt-az mode**, periodically, because field rotation changes the mapping as the Sun moves. Disconnecting the mount discards it. The manual slew pad needs no calibration (it drives the axes directly).

**ZWO AM3 notes.** The AM3 has a single guide rate (0.1–0.9× sidereal), so both axes guide at the same speed; calibration uses 0.5×. At the guide rate the image moves 0.5 × 15″/s ≈ 7.5″/s, so a 4 s calibration pulse moves it ≈ 30″: at 1″/px that is 30 px, comfortably above the 5 px minimum; at coarse scales (> 5″/px) it may be too small.

## Guiding

Every measurement: if the target is more than 5 px away on either axis, send a guide command along the (matrix-transformed, normalised) error direction at 0.5× sidereal and re-check after 1 s; once within margin, stop and re-check every 2 s. Parameters live in `GuideParams` (`crates/tracking/src/guiding.rs`); they are not yet configurable from the UI.

## Verification

The pure logic is unit-tested, including a closed-loop test (calibrate in a modelled world with drift, knock the target 60 px off, require < 12 px error after 30 s). End to end, the simulators hold the Sun within a few pixels. Real-sky behaviour (seeing, rotation, backlash) is unverified.
