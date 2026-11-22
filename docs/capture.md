# Capture & flats

Decisions: [ADR 0009](adr/0009-capture-formats-and-flats.md) (formats, flats), [ADR 0011](adr/0011-storage-selection-and-safety.md) (storage), [ADR 0012](adr/0012-burst-capture-and-ram-buffer.md) (bursts, buffer).

## Output

`<data-dir>/<UTC date>/<kind>_<UTC time>[_name].<ext>` with a `.json` sidecar (camera controls, mount and tracker state, details).

| Kind | Files | Notes |
|---|---|---|
| `frame_…` | `.png` | 8 or 16-bit grayscale |
| `seq_…` | `.ser` *or* a folder of `frame_000001.png…` | one sequence; stop by frame count and/or duration |
| `burst_…_b001` | same | one per burst of a schedule |
| `flat_…` | `.ser`, `_master.png` | N flat frames and their 16-bit mean |

Download from the **Captures** panel or `/files/<date>/<name>`.

## Bursts and the RAM buffer

Frames are captured at camera speed into a **RAM buffer** and written to disk by a separate thread, so a slow USB stick does not limit the frame rate. **Record → Bursts every N seconds** schedules "100 frames every 30 s": *frames or seconds* per burst, *every* = start-to-start spacing, *bursts* = how many (blank = until you press Stop). Live view, tracking and guiding keep running between bursts.

- Buffer size (default 1 GB) is set in the Capture panel and capped to 70 % of free memory. The panel shows whether a burst fits and the average disk load of the schedule; while recording it shows buffer fill and measured disk speed.
- If a burst is larger than the buffer, capture slows to disk speed once it is full. **Frames are not dropped.**
- After the last burst the state is *flushing* until everything is on disk; storage changes and closing the camera wait for it. Stopping the service flushes first.
- **Formats:** *SER* (one file per burst) or *PNG images* (a folder per burst, `frame_000001.png`, …). Each burst also gets a `.json` sidecar.

## Storage

Choose where captures go in the **Storage** panel: detected USB drives are listed with free space and the recording time left at the current frame rate. The choice is saved.

- A removed drive is **never silently replaced by the SD card**: captures refuse to start until it is back, and a running capture stops cleanly and keeps its frames.
- Capturing stops (cleanly) when less than 50 MB is free; starting needs 200 MB.
- **FAT32** (most factory-formatted sticks): a file cannot exceed 4 GB. RustFlare detects this and **splits SER files into `_part2`, `_part3`…**; PNG folders are unaffected. For long sequences exFAT or ext4 is better (RustFlare never formats a drive).
- The service must be able to write to the drive: after `sudo apt install` it runs as the installing user, which owns auto-mounted drives. Otherwise: `sudo rustflare-user <your-login>`.

## Flats

!!! note "Preliminary"
    Flats currently start the moment you press the button; there is no prompt. A guided workflow (cap-on reminder, adjust settings, confirm with OK, capture, restore settings, confirmation) is planned: see the [roadmap](roadmap.md). Until then, put the flat cap or diffuser on and set gain first.

1. Point at a uniform source (diffuser over the aperture, or defocused sky). Leave the optical train and focus as for your lights.
2. Choose frame count and target level (default 50 % of full scale).
3. RustFlare stops tracking/guiding, then adjusts exposure — measuring the mean level and rescaling exposure, backing off hard if the frame clips — until the mean is within ±5 % of the target. Then it records the frames, writes the master, and **restores your previous exposure**.
4. If the level cannot be reached within the camera's exposure range it stops with an explanatory error (e.g. "too bright even at minimum exposure": add more diffusion or reduce gain).

The sidecar JSON records the flat exposure and final mean level so lights and flats can be matched (same gain/offset/format!).

## Sizes

Roughly `width × height × bytes` per frame: 1280×960 at 16-bit is 2.4 MB, so 1000 frames ≈ 2.4 GB. The Storage panel shows recording time left.

## HDR bracketing lanes

For eclipses and the Moon: Record tab > HDR bracketing lanes (or `POST /api/capture/lanes`). Define 2 to 6 lanes, each with a name, an exposure (and optionally a gain) and a frame count or duration. The engine cycles through them in order, repeating for the number of cycles you give (or until stopped). Each lane records into its own folder, `<session>/lane-<name>/<name>_c001.ser` (one file per cycle) with a notes file, so every bracket can be stacked separately and merged into an HDR image afterwards. The first frames after each exposure change are discarded, and the exposure and gain you had are restored at the end. Presets for partial phase, diamond ring and totality are starting points; choose exposures at the eyepiece. Switching lanes costs a couple of frame times, so lanes are cycled rather than interleaved frame by frame.

```
curl -X POST localhost:8080/api/capture/lanes -H 'content-type: application/json' -d '{"name":"eclipse","cycles":10,"format":"ser","lanes":[{"name":"short","exposure_us":200,"frames":100},{"name":"long","exposure_us":20000,"frames":50}]}'
```

Hardware-unverified: exposure-switch latency on the Apollo-M MINI, behaviour at full rate on the stick (see the checklist).

## Colour (one-shot-colour) cameras

Colour cameras (Player One and ZWO ASI colour models report their filter layout) are handled as raw Bayer sensors:

- **Live view:** each 2x2 block of the mosaic becomes one colour pixel (a "superpixel" demosaic, no interpolation), so the preview is in colour at half the sensor resolution and costs about the same as the mono preview.
- **Recordings stay raw.** SER files are the undemosaiced mosaic with the Bayer pattern in the header (COLORID 8 to 11), which is what AutoStakkert, PIPP, Siril and others expect for planetary and lunar work; they debayer when stacking. FITS files carry `BAYERPAT`, `XBAYROFF` and `YBAYROFF`. PNG stills are raw too.
- **Tracking and guiding** run on the raw mosaic (fine for a bright disc; the Sun is found by its edge).
- **Flats and darks** are per photosite on the raw mosaic, so they correct each colour separately.
- Do not use hardware binning with a colour camera: it destroys the pattern.
- The simulator has a colour camera ("Simulated colour camera (RGGB)", id `sim_osc`) for testing.

Not done yet: a full-resolution interpolating demosaic for the preview, white balance controls, and saving debayered PNGs.
