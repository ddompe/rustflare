# File formats

Every file RustFlare writes or reads, what is in it and what reads it. Written for people who process the data with their own tools (AutoStakkert, PIPP, Siril, PixInsight, Python) and for anyone who wants to build on the files. The controls are in the [Record](../guide/record.md) and [Stacking](../guide/stacking.md) guides; the capture options and storage rules are in [Capture formats and storage](capture-formats.md).

## Where things are

```
<storage>/                           the drive's rustflare/ folder (More > Storage)
  <UTC date>/                        2026-11-22
    seq_<UTC time>[_name].ser          a sequence (or burst_<time>_<name>_b001.ser, one per burst)
    seq_<UTC time>[_name].json         its notes (sidecar)
    seq_<UTC time>[_name].ser.ready    "complete" marker
    hdr_<time>_<name>/  prominence_<time>_<name>/       a lane session
      session.json                       what the lanes were
      lane-<lane>/<lane>_c001.ser (+ .json, .ready)      one recording per lane and cycle
    flat_<time>.ser / .json / _master.png      flats;  dark_<time>.ser / .json / _master.png   darks
    frame_<time>.png (+ .json)                 single frames
  stacks/                            the stacker's output, mirroring the day folders
    <UTC date>/stack_<recording>.fits
    <UTC date>/<session>/lane-<lane>/stack_<lane>_c001.fits
    manifest.json                      one entry per finished stack
    .thumbs/                           preview pictures made by the app (cache, safe to delete)
  logs/<UTC date>/                   events.jsonl, mount.jsonl, stack.jsonl, session_*.png, lost/, debug/
```

Times in names are UTC (`150204Z`). The app's **Files** screen shows this tree and downloads a folder as one `.tar` ([API](api.md)).

Settings and state live apart from the data, in the service's settings folder (`/var/lib/rustflare` on a Pi): `rustflare.json` (the app's settings), `stack.json` (the stacker's), `capture.state`, `stack-status.json`, `stack.sock` and `tls/`. They are not on the drive and not part of a download.

## SER recordings

The raw product: one file per burst, mono, 8 or 16 bits, uncompressed, nothing applied (no dark, flat, debayer or stretch). Layout of the SER format (version 3):

| Offset | Size | Content |
|---|---|---|
| 0 | 14 | `LUCAM-RECORDER` |
| 14 | 4 | camera series id (0) |
| 18 | 4 | colour id: 0 mono, 8 to 11 Bayer RGGB, GRBG, GBRG, BGGR (raw mosaic, not debayered) |
| 22 | 4 | endianness flag: 0, and the pixels are **little-endian** (as other recorders write it, despite the spec) |
| 26, 30 | 4, 4 | width, height |
| 34 | 4 | bits per pixel plane: 8 or 16 (a 12-bit camera is stored in 16 with the values in the low 12 bits) |
| 38 | 4 | frame count (kept up to date every 50 frames, so a cut-off file still opens) |
| 42, 82, 122 | 40 each | observer (empty), instrument (camera name), telescope |
| 162, 170 | 8, 8 | local time, UTC at the start, in ticks of 100 ns since 0001-01-01 |
| 178 | w·h·bytes per frame | the frames, one after the other |
| after the frames | 8 per frame | timestamps, ticks of 100 ns since 0001-01-01 (UTC), little-endian |

A file that would pass 4 GB on a FAT32 stick is split into `name.ser`, `name_part2.ser`, … each a valid SER; the stacker reads them as one. Frame counts and fps can be checked in the app (**Check** in Files). A prominence-lane recording with the coronagraph is raw like the others: the Sun's disc is saturated, not blacked out (before 0.6 it was blacked out in the file).

## Notes file (sidecar) `<recording>.json`

Written when a burst ends. Keys: `kind` (`burst`, `sequence`, `lane`, `flat`, `dark`, `frame`, `lane-session`), `written_utc`, `camera`, `controls` (every camera control: `exposure_us`, `gain`, `offset`, …), `mount`, `mount_tracking`, `tracking` (mode, Sun centre `x`, `y` and `radius` in pixels), and `details`:

| `details` key | Meaning |
|---|---|
| `name`, `burst`, `of`, `frames`, `format` | what was asked for and how many frames were captured |
| `dropped_frames`, `stopped_early`, `duration_s` | losses and why a recording ended early (null when fine) |
| `stack` | when the burst was stacked from memory: the stack's file and numbers |
| `disc_track` | prominence lane with the coronagraph: one `[x, y, radius]` per frame, in pixels of the recorded frame, from the Sun model; radius 0 where the Sun was not in view. Frame *i* of the SER is entry *i* |

The stacker reads `controls.exposure_us` and `controls.gain` to pick the matching dark.

## `.ready` marker

`<recording>.ready` is written last, after the file is complete and flushed: `{"frames":100,"parts":1}`. The stacker only touches recordings that have one.

## Lane session `session.json`

Same top-level keys as a notes file; `details.kind` is `hdr` or `prominence`, `details.cycles`, and `details.lanes[]` with `name`, `exposure_us`, `gain`, `frames`, `seconds` and `mask_disc` (the lane that carries `disc_track`).

## Stacks `stack_*.fits`

Mono FITS images from `rustflare-stack`: 32-bit float (`BITPIX -32`) by default, or unsigned 16-bit (`BITPIX 16`, `BZERO 32768`) with `float: false`. Big-endian as FITS requires. **Rows run bottom-up** (`ROWORDER = 'BOTTOM-UP'`, the first row is the bottom of the picture, as in the recorder's FITS); tools that follow `ROWORDER`, such as Siril and PixInsight, show it upright. Values are on the camera's 16-bit scale (0 to 65535), averaged, with nothing stretched.

| Key | Meaning |
|---|---|
| `STACKED`, `NFRAMES`, `KEEPFRAC` | frames averaged, frames in the recording, fraction kept (best first) |
| `ALIGNED`, `SIGCLIP` | phase-correlation alignment on; sigma clip (0 = off) |
| `FLATCORR`, `DARKCORR` | a master flat / dark was applied (T or F) |
| `ORIGFILE` | the source recording's file name |
| `DATE-OBS` | UTC of the first frame |
| `EXPTIME`, `GAIN` | seconds per frame and camera gain, from the notes file |
| `DISCX`, `DISCY`, `DISCR` | prominence lane: the Sun's disc centre and radius **in this stack**, in pixels counted from 0 (x to the right, y counted from the **top** of the picture as the app shows it) |
| `DISCHOW` | `'fitted'` (a circle fitted to the disc's edge in the stack) or `'recorded'` (the recorder's positions, when the edge could not be found) |
| `DISCMASK`, `DISCSCAL` | a black disc was drawn (T), and its radius as a multiple of `DISCR`. When F the picture is as recorded, disc included |
| `SWCREATE` | `rustflare-stack` |

The stack is a one-recording product: its frames can be recomputed any time from the SER, so deleting a stack loses no data (use **Re-stack** in Files to have it made again).

## `stacks/manifest.json`

A JSON array, one object per stack: `source` and `output` (paths relative to the data folder and to `stacks/`), `frames`, `kept`, `signal`, `shift_rms_px`, `refine_change_px`, `clipped_fraction`, `exposure_us`, `gain`, `dark_applied`, `flat_applied`, `notes` (what was left out and why, for example "no dark matches …", "the disc's edge could not be found"), `seconds`, `phases` (time per step) and, for prominence lanes, `disc` (`x`, `y`, `r`, `how`, `masked`, `scale`). A recording with an entry is not stacked again, whether or not its FITS still exists. Do not edit it by hand: the app's Re-stack removes entries and files together.

## `stack.json` (stacker settings)

`enabled`, `policy` (`when_idle`, `between_bursts`, `always`), `min_gap_s`, `keep_percent` (1 to 100), `multipoint`, `threads` (1 to 4), `float`, `flat` and `darks` (raw recordings, relative to the data folder), `include_unmarked`, `telemetry`, `disc_mask` (draw the black disc on prominence stacks, default on) and `disc_scale` (0.8 to 1.2, default 1.01; a disc taken from the recorded positions gets at least 1.03). Missing keys take their defaults. Changing a setting affects stacks made afterwards.

## Single frames and image sequences

`frame_….png`: 8 or 16-bit grayscale PNG (colour cameras also write `_raw.png` and a debayered 16-bit colour PNG). Image-sequence recordings are a folder of `frame_000001.png` or `.fits`; FITS frames carry `DATE-OBS`, `EXPTIME`, `GAIN`, `INSTRUME`, `OBJECT` and, for colour cameras, `BAYERPAT`. Flat and dark `_master.png` files are 16-bit means for display; stackers should build their own masters from the SER.

## Logs `logs/<date>/`

| File | Content |
|---|---|
| `events.jsonl` | one JSON object per line: `event`, UTC time, state and the details of what happened (recording started, frames dropped, Sun held, lane changed, …) |
| `mount.jsonl` | every mount command and reply |
| `stack.jsonl` | the stacker's telemetry |
| `session_<time>_f00001.png` | the session log's thumbnails of the live view |
| `lost/`, `debug/` | frames saved when tracking was lost; snapshots with their state (`.json` + `.png`) |

See [Debugging and logs](debugging.md).

## Folder download `.tar`

`GET /api/files/archive?path=<folder>` streams a standard ustar archive, entries named from the folder down (`lane-disc/stack_disc_c001.fits`), files stored as they are on the drive, with the size known up front. Any `tar`, Finder, Explorer or 7-Zip opens it. Hidden files and the thumbnail cache are left out.
