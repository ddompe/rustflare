# User guide

RustFlare runs on the Raspberry Pi at the telescope and is controlled from a web page. On an iPhone, open `http://astropi.local:8080`, then Share, *Add to Home Screen* for a full-screen app, and set Auto-Lock to Never for the session.

The screen has the **live view** on top (with overlays), the five **tabs** at the bottom, and the controls of the selected tab in between. The tabs follow a session: **Camera, Aim, Track, Record, More**.

## Before the first night (More tab)

- **Site**: type your latitude and longitude (degrees, east positive; a site west of Greenwich has a negative longitude). Save. It stays on the Pi and is used for *Go to Sun*. Browsers only share location over HTTPS, so *Use this device's location* works only once HTTPS is set up.
- **Optics**: native focal length and, if you use a Barlow or reducer, its factor. RustFlare shows the effective focal length, the image scale and how large the Sun should be in pixels, and tells you when the measured Sun disagrees with what you entered.
- **Storage**: pick the drive for captures (a USB stick appears automatically). FAT32 sticks work; a file cannot exceed 4 GB, so long recordings continue in `_part2`, `_part3` files. Use a USB 3 (blue) port for the stick and a different blue port for a USB 3 camera so they do not share a controller. **Eject drive safely** flushes and unmounts the stick (and powers it off) before you pull it; it is refused while recording.
- **Clock**: the Pi's clock is used to compute where the Sun is. The Site section shows whether it is synchronised to a time server.

## A session

1. **Camera**: open the camera. Use the histogram: the bright part of the disc should sit clear of the right edge; *Clipping* means too much exposure or gain. Exposure and gain take typed values or the slider (it is logarithmic, so a few milliseconds are easy to hit).
   - **Flats** (Camera tab): *Start flats*, put the flat cap or diffuser on, check the level, press *Capture*. Exposure is found automatically (about 50 %), the master flat is written and applied to the live view; the overlay shows *Flat: applied*. Recordings stay raw. The switch turns the correction on or off; the master flat is remembered across restarts.
2. **Aim**: connect the mount (USB or Bluetooth; for Bluetooth close the ZWO phone app and press the mount's pairing button). The state tags show the link and what the mount reports.
   - **Go to Sun** slews to where the Sun is now (from your site and the Pi's clock), then switches on solar tracking by itself. It asks for confirmation; STOP cancels. If the Pi's clock is not synchronised it warns you.
   - **Send home** goes to the mount's home position, with confirmation.
   - The N/S/E/W pad moves while held (arrow keys work too); the slew speed is remembered. **Solar tracking** follows what the mount itself reports.
3. **Track**: *Disc edge (limb)* works for a whole or partial disc as long as some limb is in view. When the field is entirely on the disc, use *Feature (anchor)* and tap a sunspot or plage. **Calibrate** (needs mount solar tracking) teaches RustFlare which way each axis moves the image; **Guide** then corrects slow drift only, not seeing.
4. **Record**: single frames, a **sequence**, or **bursts** ("100 frames every 30 s"). Frames go into a RAM buffer and are written to disk in the background; the camera never slows down for the disk until the buffer is full.
   - **Captures** lists what you recorded. *Check* reads a recording's header and samples frames: size, frame count, measured fps, duration and brightness, and flags **Mostly dark** (cap left on, Sun out of view) and **Incomplete** (cut-off). *Download* and *Delete* (with confirmation) work per capture; deleting a capture removes its notes file too.

## The live view and its overlays

The overlays sit on the picture, are semi-transparent, can be dragged anywhere, and tap to shrink to one line. The grid button in the picture's tool column switches each one on or off and sets their opacity.

- **Status**: camera fps, screen fps (the preview rate your phone receives), disk MB/s while recording, exposure, gain, sensor temperature, mount state, and whether a flat is applied.
- **Recording** and **RAM buffer**: appear while recording.
- **Guiding**: the average error and a one-minute graph while tracking.
- **Histogram**.

The round **Prominence view** button in the picture's tool column blacks out the Sun's disc (display only) so you can judge the exposure you want for prominences: raise exposure or gain and watch the faint structure at the limb appear. It does not auto-stretch, so what you see is the real brightness; use *Auto-stretch* in Display if you want it brightened. *Record > Prominence pair* then captures the disc and a longer exposure together.

The slider button opens display settings (auto-stretch, brightness, contrast, gamma, colour tint, bullseye). These affect viewing only, not what is recorded.

## When something goes wrong

- A red banner shows the last problem (camera error, lost target, storage unavailable, mount error, lost mount link).
- *No Sun found*: too dark (raise exposure) or no limb in view; use the anchor tracker.
- *Target lost*: the tracker keeps retrying; guiding pauses until it recovers.
- *Mount link lost*: the USB mount is found again automatically; for Bluetooth press the pairing button again.
- *Storage not available*: the drive is not mounted; reconnect it or choose another in Storage.
- *Go to Sun refuses*: set the site, check the Sun is above the horizon, check the clock.
