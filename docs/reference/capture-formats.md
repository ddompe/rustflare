# Capture formats and storage

How recordings are stored and what is in them. For the controls, see the [Record tab](../guide/record.md). Decisions: [ADR 0009](../adr/0009-capture-formats-and-flats.md) (formats, flats), [ADR 0011](../adr/0011-storage-selection-and-safety.md) (storage), [ADR 0012](../adr/0012-burst-capture-and-ram-buffer.md) (bursts, buffer).

## Output

Every file format in detail (SER layout, notes file, stack FITS headers, manifest, logs): [File formats](file-formats.md).

`<data-dir>/<UTC date>/<kind>_<UTC time>[_name].<ext>` with a `.json` sidecar (camera controls, mount and tracker state, details).

| Kind | Files | Notes |
|---|---|---|
| `frame_…` | `.png` | 8 or 16-bit grayscale |
| `seq_…` | `.ser` *or* a folder of `frame_000001.png…` | one sequence; stop by frame count and/or duration |
| `burst_…_b001` | same | one per burst of a schedule |
| `flat_…` | `.ser`, `_master.png` | the raw flat frames, and their 16-bit mean (a display convenience; stackers should build their own master from the SER) |
| `dark_…` | `.ser`, `_master.png` | the raw dark frames (kept since 0.4.1; earlier darks have only the master), and their 16-bit mean |
| `*.fits` | one file per frame in a folder | header has time, exposure, gain, instrument |

Download from the **Files** screen (see [Record](../guide/record.md#files-file-manager)) or `/files/<date>/<name>`.

## Bursts and the RAM buffer

Frames are captured at camera speed into a **RAM buffer** and written to disk by a separate thread, so a slow USB stick does not limit the frame rate. **Record → Bursts every N seconds** schedules "100 frames every 30 s": *frames or seconds* per burst, *every* = start-to-start spacing, *bursts* = how many (blank = until you press Stop). Live view, tracking and guiding keep running between bursts.

- Buffer size (default 1 GB) is set in the Capture panel and capped to 70 % of free memory. The panel shows whether a burst fits and the average disk load of the schedule; while recording it shows buffer fill and measured disk speed.
- If a burst is larger than the buffer, then once it is full capture either slows to disk speed (*Wait for the disk*, no frame is lost) or keeps full speed and drops frames (*Drop frames*): the oldest frames still waiting for the disk are overwritten by the camera and counted. A recording that lost frames says so in four places: the live count in the recording status, a note on the Record tab until the next recording starts, an entry in the event log (`frames_dropped`) and the burst's `.json` (`dropped_frames`, and `frames_written` next to the number captured).
- After the last burst the state is *flushing* until everything is on disk; storage changes and closing the camera wait for it. Stopping the service flushes first.
- **Formats:** *SER* (one file per burst), *PNG* or *FITS* images (a folder per burst). Each burst also gets a `.json` sidecar.

## Storage

Choose where captures go in the **Storage** panel: detected USB drives are listed with free space and the recording time left at the current frame rate. The choice is saved.

- **The Pi's own SD card** is offered in the Storage panel as *Internal storage (SD card)*. Use it for a short, fast event or for the stack-only and best-frames modes, not for long full-rate recordings: it writes at roughly 30 to 90 MB/s, so the RAM buffer fills in seconds. A **reserve** (8 GB by default, 2 to 100 GB, set in the panel) is always kept free for the system, its logs and settings: a recording will not start without it and stops cleanly before it would use it, counting the frames still in memory that have yet to be written (`PUT /api/storage/reserve`).
- A removed drive is **never silently replaced by the SD card**: captures refuse to start until it is back, and a running capture stops cleanly and keeps its frames.
- Capturing stops (cleanly) when less than 50 MB is free; starting needs 200 MB.
- **FAT32** (most factory-formatted sticks): a file cannot exceed 4 GB. RustFlare detects this and **splits SER files into `_part2`, `_part3`…**; PNG folders are unaffected. For long sequences exFAT or ext4 is better (RustFlare never formats a drive).
- The service must be able to write to the drive: after `sudo apt install` it runs as the installing user, which owns auto-mounted drives. Otherwise: `sudo rustflare-user <your-login>`.

## Flats

1. Point at a uniform source (diffuser over the aperture, or defocused sky). Leave the optical train and focus as for your lights.
2. Choose frame count and target level (default 50 % of full scale).
3. RustFlare stops tracking and guiding, then adjusts exposure, measuring the mean level and rescaling exposure, backing off hard if the frame clips, until the mean is within ±5 % of the target. Then it records the frames, writes the master, and **restores your previous exposure**.
4. If the level cannot be reached within the camera's exposure range it stops with an explanatory error (e.g. "too bright even at minimum exposure": add more diffusion or reduce gain).

The sidecar JSON records the flat exposure and final mean level so lights and flats can be matched (same gain/offset/format!).

## Sizes

Roughly `width × height × bytes` per frame: 1280×960 at 16-bit is 2.4 MB, so 1000 frames ≈ 2.4 GB. The Storage panel shows recording time left.

## HDR bracketing lanes

For eclipses and the Moon: Record tab > HDR bracketing lanes (or `POST /api/capture/lanes`). Define 2 to 6 lanes, each with a name, an exposure (and optionally a gain) and a frame count or duration. The engine cycles through them in order, repeating for the number of cycles you give (or until stopped). Each lane records into its own folder, `<session>/lane-<name>/<name>_c001.ser`: one short raw video per lane and cycle, with a notes file holding the exposure and UTC times. Stack each video with lucky imaging into one frame; the stacks of one lane, in cycle order, form a time-lapse, and the lanes merge into HDR frames. Add `cycle_interval_s` ("Start a new cycle every" in the app) for a steady cadence, for example a new cycle every 60 s. Sessions are saved as `hdr_<time>/` or, with `"kind":"prominence"`, `prominence_<time>/` (a `disc` lane and a longer `prominence` lane), each with a `session.json` describing the lanes. The first frames after each exposure change are discarded, and the exposure and gain you had are restored at the end. Presets for partial phase, diamond ring and totality are starting points; choose exposures at the eyepiece. Switching lanes costs a couple of frame times, so lanes are cycled rather than interleaved frame by frame.

```
curl -X POST localhost:8080/api/capture/lanes -H 'content-type: application/json' -d '{"name":"eclipse","cycles":10,"format":"ser","lanes":[{"name":"short","exposure_us":200,"frames":100},{"name":"long","exposure_us":20000,"frames":50}]}'
```

!!! note "Status"
    Built and tested on the simulator. Exposure-switch latency on a real camera and behaviour at full rate on a USB stick have not yet been measured.

## Colour cameras

Colour cameras (Player One and ZWO ASI colour models report their filter layout) are handled as raw Bayer sensors:

- **Live view:** each 2x2 block of the mosaic becomes one colour pixel (a "superpixel" demosaic, no interpolation), so the preview is in colour at half the sensor resolution and costs about the same as the mono preview.
- **Recordings stay raw.** SER files are the undemosaiced mosaic with the Bayer pattern in the header (COLORID 8 to 11), which is what AutoStakkert, PIPP, Siril and others expect for planetary and lunar work; they debayer when stacking. FITS files carry `BAYERPAT`, `XBAYROFF` and `YBAYROFF`. A single-frame PNG is saved both raw (`_raw.png`) and as a debayered (bilinear), white-balanced 16-bit colour PNG.
- **Tracking and guiding** run on the raw mosaic (fine for a bright disc; the Sun is found by its edge).
- **Flats and darks** are per photosite on the raw mosaic, so they correct each colour separately.
- Do not use hardware binning with a colour camera: it destroys the pattern.
- The simulator has a colour camera ("Simulated colour camera (RGGB)", id `sim_osc`) for testing.

Not available yet: a full-resolution interpolating demosaic for the preview, white balance controls, and saving debayered PNGs.
