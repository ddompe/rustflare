# Camera tab

![Camera tab](../img/manual/camera.png)

!!! note "Screenshot"
    The picture is from 2026-11-21: it has no profile picker, optical setup line, *Set up N of 5* strip, colour histogram, HCG marks or cooler section. The text is current.

At the top of the tab (once the camera is open) is the profile picker, *What are you imaging?* (see [profiles](profiles.md)), and, when an optical setup is in use, a line naming it that opens the setups dialog.

Open the camera, then set up the picture.

## Exposure and gain

Exposure and gain take typed values or the slider. The exposure slider is logarithmic, so a few milliseconds are easy to hit. You can also choose the bit depth.

Use the **histogram**: the bright part of the disc should sit clear of the right edge. A *Clipping* warning means too much exposure or gain.

With a **colour camera** the histogram shows three curves, red, green and blue, drawn over each other in their own colours. Each channel is counted from its own photosites of the raw mosaic, so a red curve pushed against the right edge means the red channel is clipping even when the picture looks fine. A coloured marker at the right edge appears for every channel that clips, and the message names them (*R 1.2%, B 0.4%*). A mono camera shows the single blue-grey histogram as before. On the Planet profile, clipping means the brightest part of the picture (the planet) reaches full scale; see [profiles](profiles.md).

Under the gain slider, **HCG** and **Unity** marks show where the camera's high conversion gain switches on and where one electron equals one count, when the [device catalog](../reference/hardware.md) knows them for your camera (the ZWO ASI533MC Pro: both at gain 100). Gain at or above the HCG mark gives much lower read noise, which suits short exposures of the Sun, Moon and planets. The mark's tooltip says where the number comes from.

## Cooler (cooled cameras)

A camera with a thermoelectric cooler (for example the ZWO ASI533MC Pro) gets a **Cooler** section. It has:

- the **Cooler** switch and a **Target temperature** slider (the range comes from the camera), with the sensor temperature, the **cooler power** in percent and a short history graph (about ten minutes, dashed line is the power);
- **Fan** and **Anti-dew heater** switches, shown only when the camera has them.

Cooling helps long exposures; for the few milliseconds of a Sun or planet frame it changes little, so leaving it off is fine. Set the target first, then switch on: the camera needs a few minutes to get there and the power readout settles when it has.

**Switching off is gentle.** While the sensor is cold (below 15 degrees) a request to switch the cooler off raises the target 1 degree every 15 seconds (about 4 degrees a minute) and switches off when the sensor is near room temperature, or when the cooler is idle. The switch reads *Warming the sensor before switching off*, and **Switch off now** cuts the cooler at once if you must. A sensor warmed too fast is stressed and its window mists; do not cover or open a cold camera. The cooler is not switched on by itself, and closing the camera in RustFlare leaves it as it was.

The simulator has a cooled colour camera ("Simulated cooled colour camera (RGGB)") for trying it out; its sensor moves about a degree a second.

## Frame size

**Frame size** reads out only the centre 75, 50 or 25 % of the sensor. That gives a higher frame rate for the same USB bandwidth, for example about 150 fps instead of 57 on one Player One camera (see [Performance](../reference/performance.md)). It works on Player One and ZWO ASI cameras. Tracking restarts when you change it.

## Flats

*Flats and darks are on the [Record tab](record.md#flats-and-darks)* (they are everyday work, done with the recordings); the steps are here.

1. Press *Start flats...* (Record tab > Flats).
2. Put the flat cap or a diffuser on and check the level.
3. Press *Capture*.

The exposure is found automatically (about 50 % of full scale). The master flat is written and applied to the live view, and the Status overlay shows *Flat: applied*. Recordings stay raw. A switch turns the correction on or off, and the master flat is remembered across restarts. Details: [Capture formats](../reference/capture-formats.md#flats).

## Darks

Cap on, the same exposure and gain as on the Sun, then *Capture darks*. The master dark is subtracted from the live view (before the flat). The Status overlay shows *Dark: applied*, or *mismatch* if the camera's settings changed since. Recordings stay raw.

## Colour cameras

Colour cameras show a colour live view (half resolution) with white balance (Camera tab > Colour: **Auto** makes the bright disc neutral, **Manual** sets red and blue gains, **Off** shows the sensor's own colours). A single-frame PNG from a colour camera is saved twice: `..._raw.png` (the mosaic) and a debayered, white-balanced 16-bit colour `.png`. Recordings stay raw (undemosaiced, with the Bayer pattern in the file), which is what stacking software such as AutoStakkert, PIPP and Siril expects. Do not use hardware binning with a colour camera. See [Capture formats](../reference/capture-formats.md#colour-cameras).

## Reconnecting

*Reconnect camera and mount at startup* (More > Startup) makes RustFlare reopen the last devices after a restart. See [Getting started](getting-started.md#what-the-service-does-by-itself).

## Focus assist

Camera tab > Focus assist helps you focus by hand, which is hard to judge by eye on a bright day. Switch it on and turn the focuser slowly. A **score** appears over the live image together with a small **history graph**, and again in the Camera tab with a larger graph, the best score so far and how long ago you reached it.

- The score is a sharpness measure of the Sun's **surface detail** (filaments, plage, granulation) inside the disc. The limb is left out, because turning a focuser shifts the Sun a little and the limb would move through the window and fool the score. The window stays where the Sun was when you switched focus assist on. The score does not change with exposure or gain.
- Seeing makes single frames jump, so each burst of about 2.5 seconds is summarised by the mean of its **sharpest 12 %** of frames (at least three), with the scatter of those frames as an **error**. One point of the graph is one burst, and the grey band around the line is its error: a difference smaller than the band is noise, and the app says "same (within noise)" instead of sharper or softer.
- The grey band is the larger of two things: how far the best frames of one burst differ from each other, and how much neighbouring bursts differ from each other (the seeing), measured from the last 12 bursts with a steady climb or fall taken out. On the real Sun the second is about four times the first, so it is the one you see.
- The dashed **best** line is the peak of the *smoothed* curve, so one lucky burst does not win. Turn towards the top of the curve, pass it, and come back to the best line; "you are at the best" appears when you are within the noise of it. **Reset history** starts a new session, for example after changing the filter or after a Go to (the window is placed again).
- Keep solar tracking on, so the Sun stays roughly in place, and keep the Sun in view with the cap off. If nothing bright is in view, or the Sun has left the window, the panel says so.
- The simulator has a *Sim defocus* control for trying it without a telescope.

Status: tested on the simulator; the score has not yet been tuned on real solar data (see the hardware checklist).

**Close camera** asks first when a recording, a calibration or guiding is running, and says what closing will end.
