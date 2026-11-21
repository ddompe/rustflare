# Using Rustflare

A typical session with an H-alpha scope that shows part of the disc.

1. **Camera**: pick your camera, *Open*. Check the **histogram**: the bright part of the disc should sit well clear of the right edge; the red *Clipping* warning means too much exposure or gain. Exposure and gain take typed values; ½× / 2× buttons halve and double exposure.
2. **Optics** (once per setup): enter the telescope's native focal length and, if you use a Barlow or focal reducer, its factor (2 for a 2× Barlow, 0.63 for a reducer): Rustflare shows the effective focal length. The factor is nominal (the real magnification depends on the Barlow's spacing from the sensor), so check that the measured Sun radius matches the expectation; a mismatch is flagged (and pixel size if the camera does not report it; binning if you bin). Rustflare shows the image scale and how big the Sun should be in pixels today. Partial-disc tracking uses this.
3. **Mount**: driver `zwo`, device (e.g. `/dev/ttyACM0`), *Connect*. Tick **Solar tracking** (selects the solar rate, then enables tracking). Use the N/S/E/W pad (hold to move) to bring the target into view.
4. **Look at it**: **Display** panel: *auto-stretch* brightens dim 16-bit frames; brightness/contrast/gamma, invert and the H-alpha orange tint are for viewing only (they do not change what is recorded). Wheel zooms, drag pans, double-click resets. The bullseye rings and crosshair help centring; the cyan outline is the detected Sun and the orange arrow points from it to the frame centre.
5. **Track**: *Disc edge (limb)* works for a whole or partial disc as long as some limb is in view. If the field is entirely on the disc (high magnification), use *Feature (anchor)*: press *Pick feature…* and click a sunspot, filament or plage.
6. **Calibrate** (needs mount solar tracking on): the mount moves each axis a few seconds and Rustflare learns which way each moves the image. See [Calibration](tracking-and-guiding.md#calibration).
7. **Hold or centre**: with a partial disc, slew the region you want into view and press **Hold position**. **Centre & guide** moves the disc centre to the middle of the frame, which only makes sense when that is inside the field.
8. **Capture**: single frames (PNG), a **sequence**, or **bursts** ("100 frames every 30 s"). Frames are captured into the RAM buffer and written in the background. Choose SER (one video file per burst) or PNG images (a folder per burst). See [Capture](capture.md).

Everything captured is listed under **Captures** and downloadable.

## When something goes wrong

- A red banner shows the last problem (camera error, lost target, storage unavailable, mount error).
- *No Sun found*: too dark (raise exposure), or no limb in frame, in which case use the anchor tracker.
- *Target lost*: the tracker keeps retrying; guiding is paused until it recovers.
- *Calibration failed*: check mount solar tracking is on and the Sun is well inside the frame.
- *Storage not available*: the chosen drive is not mounted; reconnect it or pick another in **Storage**.
