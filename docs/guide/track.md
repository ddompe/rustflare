# Track tab

![Track tab](../img/manual/track.png)

The mount's own tracking follows the Sun's rate. RustFlare's tracker watches the picture and its guider corrects the slow drift that is left. It does not try to correct seeing.

## Choose what to follow

- **Disc edge (limb)**: the default. Works for a whole or a partial disc as long as some limb is in view.
- **Feature (anchor)**: when the field is entirely on the disc, tap a sunspot, filament or plage to follow it.
- **Full disc (centroid)**: when the whole disc is in the frame.

The live view shows the target (red rings) and where the Sun is (cyan), with the correction arrow. More about the modes in [Tracking and guiding](../reference/tracking-and-guiding.md).

## Calibrate

**Calibrate** teaches RustFlare which way each mount axis moves the image. It needs the mount's solar tracking on. Do it once, and again after rotating the camera, after a meridian flip, or periodically in alt-az mode.

## Guide

Press **Guide** to keep the Sun at the target. *Hold position* keeps it where it is now; centring moves it to the middle of the frame. *Guiding settings* tune how strongly it corrects. Guiding needs a calibration.

## If it loses the Sun

- *No Sun found*: too dark (raise exposure) or no limb in view. Use the anchor tracker.
- *Target lost*: the tracker keeps retrying and guiding pauses until it recovers.

## Optics

If the measured Sun size disagrees with what you entered in [More > Optics](more.md#optics), the app shows the real focal length or Barlow factor and offers *Use measured*.
