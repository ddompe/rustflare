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

## One model of where the Sun is

Everything that needs the Sun's position uses one model: the Sun outline, the tracking circle, the coronagraph mask and the guider. It is a smoothed centre and radius with a drift velocity, corrected only by measurements that agree with the prediction:

- a measurement close to the prediction counts in full, one further away counts less, one far away is set aside (shadows, cloud, a flapping fit),
- if the set-aside measurements keep agreeing with each other for a while the Sun really moved (a slew, a re-aim) and the model re-locks there,
- the drift is the least-squares slope of the last 90 s of accepted measurements, and a long-term drift is learned in good conditions (it turns over with a meridian flip),
- with no usable measurement at all it keeps predicting, so the mask survives a cloud, a tree or an eclipse.

The detector under it is brightness-independent: dim frames are brightened after removing the sensor's black level, the limb is the edge between the black sky and anything brighter (shadows included), and the limb points are filtered by RANSAC so a shadow edge cannot pull the circle. Light that drops a hundredfold in seconds (palm-tree shadows, measured on 2026-11-22) still gives the same circle.

## Calibration

**Yes: the mount's axes have to be mapped to the image once.** Which way "RA+" moves the image depends on how the camera is rotated on the scope, so RustFlare measures it instead of assuming. With solar tracking on, it looks at the Sun for 1.5 s, guides the primary axis at 0.5× sidereal for 4 s, waits 1.5 s, and measures how the position moved; then the same for the secondary axis. Start and end positions are **medians** of the last second or so, so one bad frame cannot spoil it. The two unit vectors give the image→axes matrix and the two rates (image pixels per second at 1× sidereal). It fails with a message if the image moves less than 5 px, or if the axes come out nearly parallel. The result is shown in the Tracking panel.

**The mount ends a guide command by itself after 3 seconds** (the AM3 takes `:Mge3000#`-style timed pulses). The calibrator therefore re-issues its move every 2.5 s; before 2026-11-22 it held one command for 4 s and under-read the rates by a quarter.

**When to recalibrate.** Redo it after rotating the camera or diagonal, after a **meridian flip** (the directions turn over: the real mount's image rotated by about 209 degrees), and, in alt-az mode, periodically. Disconnecting the mount discards it.

**ZWO AM3 notes.** The AM3 has a single guide rate (0.1–0.9× sidereal), so both axes guide at the same speed; calibration uses 0.5×. Measured on the real mount: about 15–19 px/s per 1× on the primary axis and 8–14 on the secondary at 0.78″/px.

## Guiding

The guider corrects the mount's drift, not the seeing: it averages the position over a window (10 s), ignores offsets inside a dead band (3 px) and sends gentle pulses (0.2× sidereal; 0.8× while the offset is over 20 px), one axis at a time, then waits for a fresh average. Pulses are chained commands of at most 2.5 s each, because the mount stops a longer one by itself.

- **It only sees trusted positions.** Measurements the Sun model rejects never reach it.
- **Drift feed-forward.** While guiding works, the guider learns the mount's *unguided* drift (position minus the motion its own pulses caused, over the last few minutes). Corrections then aim where the Sun will be half-way to the next judgement, so a steady drift does not leave a steady offset.
- **Dead reckoning.** If the Sun cannot be seen (cloud, trees, an eclipse) and the drift is known, the guider keeps running on the last good position moved on by that drift and by its own pulses. It gives up and pauses after 15 minutes or 300 px of corrections.
- **Lane recordings.** Only the shortest non-masked lane (the disc lane) is measured, in short bursts with long gaps. The guider averages each burst and judges once when it ends; the pulse then runs during the long-exposure lane. A large offset (over 40 px) must be seen in two bursts before it is acted on, because the scope has been seen sliding hundreds of pixels and coming back on its own. The long-exposure frames move the Sun model between bursts by their frame-to-frame displacement, so the outline and the mask stay on the image.
- **Tuning** is under *Guiding settings* in the Track tab.

## Status

Tested against simulators, including closed loops with drift, a shadowed disc, a Sun hidden for minutes and lane recordings. On a real sky (2026-11-22, white-light and H-alpha) the old version let tracker outliers into the guider (median error 37 px, spikes to 1470 px in shadows); this version has been through one session. After the polar alignment was corrected the measured unguided drift fell from about 1.5 px/s to 0.14 px/s. Field rotation and backlash have not been studied.
