# Tracking & guiding

How RustFlare follows the Sun and corrects drift. Controls: [Track tab](../guide/track.md). Decisions and rationale: [ADR 0008](../adr/0008-solar-tracking-and-guiding.md).

## Tracking modes

| Mode | Use when | How it works | Limits |
|---|---|---|---|
| **Disc edge (limb)** — default | whole or partial disc, some limb in view | fits a circle to the visible limb; the radius is locked (from you, the Optics setting, or the first fit) and only the centre is refined | needs a reasonable arc of limb; a radius guessed from a short arc is imprecise, so set Optics |
| **Feature (anchor)** | field entirely on the disc, or following a sunspot/filament/plage | 64×64 block matching within ±20 px, coarse-to-fine | needs structure with some spatial correlation; the reference block is not refreshed |
| **Full disc (centroid)** | whole disc in frame | centroid of the bright region, plus disc radius | the whole disc must be visible, else the centroid is biased |

**Finding the disc.** Sky and disc are separated with an Otsu threshold on a log-intensity histogram, which copes with H-alpha images where bright plage and prominences raise the peak and dark filaments lie inside the disc. If the frame does not show two clearly different populations (e.g. a field wholly on the disc) RustFlare says there is no limb in view. See [ADR 0013](../adr/0013-partial-disc-first-tracking-and-optics.md).

**Optics.** Focal length (with an optional Barlow/reducer factor, so you do not have to work out the effective focal length yourself) and pixel size give the image scale and, with the Sun's apparent size today (≈ 943–976″), the expected radius in pixels, which makes partial-disc fits much more reliable. A measured radius that disagrees by more than 10 % is flagged (wrong focal length, reducer, binning).

Frames where the measurement jumps by more than half a disc radius are treated as artefacts and ignored. If the target disappears the tracker reports *lost* and keeps trying.

### What magnification does my Barlow or reducer really give?

The factor printed on a Barlow or reducer is only nominal. It holds for one spacing between the lens and the sensor: extra extension between a Barlow and the camera raises the real factor, and a reducer gives its stated factor only at its specified back-focus distance. Your optical train decides, so RustFlare measures it instead of trusting the label:

- Whenever part of the Sun's limb is in view, it fits the limb and compares the Sun's **measured radius in pixels** with the radius your Optics entry predicts. If the data disagree, the data win (the fit residual of both radii is compared), and the Optics panel shows the **effective focal length and the real Barlow/reducer factor** that explain what the camera sees, for example *entered 3.0×, measured 3.35× (≈ 1170 mm)*.
- *Use measured* applies that factor to the Optics setting, so the image scale, the expected Sun size and the centring aids agree with reality.
- If the measured factor is higher than the label says, the lens-to-sensor spacing is larger than the design spacing; shortening it moves the factor back toward nominal (and the reverse if it is lower).

This needs the telescope's native focal length and the camera's pixel size (reported by the camera) to be entered correctly, and some limb in view. API: `status.optics.measured_radius_px`, `suggested_effective_focal_mm`, `suggested_multiplier`.

## Calibration

**Yes: the mount's axes have to be mapped to the image once.** Which way "RA+" moves the image depends on how the camera is rotated on the scope, so RustFlare measures it instead of assuming. With solar tracking on, RustFlare guides the primary axis at 0.5× sidereal for 4 s, waits 1 s, and measures how the tracked position moved; then the same for the secondary axis. The two unit vectors give the image→axes matrix. It fails with a message if the image moves less than 5 px, or if the axes come out nearly parallel. The result is shown in the Tracking panel (the image direction of RA+ and Dec+) so you can sanity-check it.

**When to recalibrate.** The mapping is fixed while the camera orientation and the mount's configuration stay the same. Redo it after rotating the camera or diagonal, after a **meridian flip** (the directions reverse), and, if the AM3 is used in **alt-az mode**, periodically, because field rotation changes the mapping as the Sun moves. Disconnecting the mount discards it. The manual slew pad needs no calibration (it drives the axes directly).

**ZWO AM3 notes.** The AM3 has a single guide rate (0.1–0.9× sidereal), so both axes guide at the same speed; calibration uses 0.5×. At the guide rate the image moves 0.5 × 15″/s ≈ 7.5″/s, so a 4 s calibration pulse moves it ≈ 30″: at 1″/px that is 30 px, comfortably above the 5 px minimum; at coarse scales (> 5″/px) it may be too small.

## Guiding

Every measurement: if the target is more than 5 px away on either axis, send a guide command along the (matrix-transformed, normalised) error direction at 0.5× sidereal and re-check after 1 s; once within margin, stop and re-check every 2 s. The strength of the correction can be tuned under *Guiding settings* in the Track tab.

## Status

The logic is tested against simulators, including a closed-loop test with drift. Guiding on a real sky (seeing, field rotation, backlash) has not yet been verified.
