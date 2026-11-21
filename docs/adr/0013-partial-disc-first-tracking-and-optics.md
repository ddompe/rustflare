# 0013. Partial-disc-first tracking, Otsu detection, and the optics setting

- Status: Accepted. Tested on synthetic H-alpha-like frames and on the simulator; not yet on real solar images.
- Date: 2026-11-21

## Context

The first target scope is an **H-alpha** telescope whose field shows only part of the disc. That changes the tracking problem from "centre the disc" to "find the disc from a short arc of limb, or follow a feature, and hold the framing". It also changes the images: bright plage and prominences push the peak intensity far above the typical disc level, and dark filaments and spots sit inside the disc. A threshold at half of the peak (the first implementation) can land *inside* the disc.

## Decision

- **Limb tracking is the default** (and is also better than a centroid for a complete disc). It finds limb points (excluding edges that are only the frame border), fits a circle (Kåsa), then locks the radius and refines only the centre (Gauss–Newton). The radius comes, in order of preference, from the user, from the optics (below), or from the first-frame fit.
- **Otsu threshold on a log-intensity histogram** separates sky from disc. Log scale because structure on the disc is multiplicative (plage, filaments), so sky-vs-disc outweighs dark-disc-vs-plage; the middle of the plateau of equally good cuts is used so a clean sky/disc gap does not put the threshold at the sky level. A frame must show two clearly different populations (the bright one at least twice as bright as the dark, and not dark in absolute terms); otherwise there is **no limb in view** and tracking reports that, instead of inventing a circle. A field entirely on the disc needs the feature (anchor) tracker.
- **Optics setting** (focal length, pixel size from the camera or typed, binning) gives the plate scale and, with the Sun's apparent radius for the day of the year (≈ 943–976″, first-order orbit model, ≈ 0.1 % accuracy), the **expected solar radius in pixels**. A fixed, known radius makes a short-arc fit far more reliable. A mismatch between the measured and expected radius is flagged in the UI (wrong focal length, reducer, binning).
- **The Sun is located on every analysed frame**, tracked or not (`locate_sun`): whole disc → centroid; partial → limb fit. This drives the centring aids (outline, offset readout, arrow) without starting a tracker.
- **UI follows the use case:** "Centre & guide" moves the disc centre to the frame centre (only sensible if that is in the frame); **Hold position** keeps the current framing, which is what a partial-disc user wants after slewing a region of interest into view. When the disc centre is outside the frame the panel says so.

## Found on real data: the optics can be wrong, so the data has the last word

On the first real H-alpha recordings the radius expected from the entered optics (350 mm x a nominal 3x Barlow = 1099 px) fitted the limb points with 21 px RMS, while a free fit gave 2.96 px RMS at 1227 px: the real effective focal length was ~1170 mm (a Barlow's true factor depends on its spacing from the sensor). Forcing the wrong radius gave a *stable but wrong* centre (about 140 px off). So the fixed-radius fit is used only if it explains the limb about as well as a free fit (RMS <= 1.5x the free RMS + 1.5 px); otherwise the radius is measured from the data, a running median of those measurements becomes the hint (the radius-centre coupling of a short arc makes single-frame free fits noisier: centre sigma 2.5/6.7 px versus 1.8/2.4 px with a fixed radius), and the UI suggests the corrected effective focal length and Barlow factor. A geometric (Gauss-Newton) refinement of the free fit was added and changed little: the algebraic fit was not the problem.

## Alternatives considered

- *Ellipse fit*: the Sun is round to far better than our pixel precision.
- *Edge detection (Canny/Hough)*: heavier, and the soft H-alpha limb with spicules favours a threshold-and-fit approach.
- *Cross-correlation against a stored reference*: that is the anchor tracker; limb fitting needs no reference.

## Consequences

- Accuracy of limb fitting depends on arc length; the fixed radius is the main lever, hence the optics panel.
- The day-of-year radius model is sufficient for tracking, not for astrometry.
- Thresholds assume the sky is darker than the disc; a very bright sky (daytime scatter with a weak filter) may need a different approach.
