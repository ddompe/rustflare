# Proposal: a file layout that explains itself, and a catalog that ties recordings to their calibration

Status: proposal, for review. Nothing here is built.

## Why

- Recordings go to `<storage>/<date>/` and darks and flats sit among them. Nothing in a folder or file name says which camera or optical setup made a file; only the JSON note does.
- One master dark and one master flat are active for the whole app, whatever the camera. A dark is judged against the camera only by exposure and gain (and the pixel size); not by region position, offset, bit depth, temperature, optical setup or camera.
- The data travels on a USB key to a laptop with no RustFlare. Folders and file names have to explain themselves there, and a tool on the laptop must not have to guess which dark belongs to which video.

## Layout

```
rustflare/
  2026-11-28/
    saturn_533MC_c11-barlow2x/        target _ camera tag _ optical setup tag
      saturn_041233_001.ser           (+ .json note, as now)
      catalog.json                    what this folder's recordings need (below)
  calibration/                        a library per camera, reused across nights
    533MC/
      darks/ dark_g100_o30_10ms_16bit_full_m10C_20261128.ser (+ .json)
      flats/ flat_c11-barlow2x_g100_o30_16bit_full_20261128.ser (+ .json)
    apollo/
  stacks/2026-11-28/saturn_533MC_c11-barlow2x_<...>.png
  logs/
```

- The folder inside a day is named from what is active when Start is pressed. The same target, camera and setup twice in a day go to the same folder; the time is in the file name.
- Tags (camera, optical setup) are short, ASCII, without spaces (FAT32 and exFAT safe, short paths), and editable in the app.
- File names carry the match key, so a file is recognisable without opening its note. The note stays the source of truth.
- Existing data stays where it is: the app reads old and new layouts, and nothing is moved automatically (moving big files is what we want to avoid). An optional "organise old folders" command could come later.

## The catalog: `catalog.json` in every recording folder

One small file that says what each recording is and which library files calibrate it. Paths are relative to the folder and point into the library (no copies, no duplication). Sketch:

```json
{
  "version": 1,
  "camera": {"tag": "533MC", "name": "ZWO ASI533MC Pro"},
  "setup":  {"tag": "c11-barlow2x", "name": "C11 + Barlow 2x"},
  "recordings": [
    {"file": "saturn_041233_001.ser", "order": 1, "start_utc": "...", "frames": 9000,
     "exposure_us": 8000, "gain": 100, "offset": 30, "bit_depth": 16,
     "roi": {"x": 1248, "y": 1248, "width": 512, "height": 512}, "sensor_temp_c": 18.4,
     "dark": {"file": "../../calibration/533MC/darks/dark_....ser", "match": "exact", "size": 123456789},
     "flat": {"file": "../../calibration/533MC/flats/flat_....ser", "match": "exact", "size": 98765432},
     "quality": {"frames_scored": 9000, "top20_mean": 17.3}}
  ]
}
```

- **Written by the app** after each recording: it picks the best library match and records how good it is (`exact`, or `nearest` with what differs, or `none` with the reason).
- **Refreshed later**: darks are often taken after the session. A "refresh catalog" action (and a command-line tool) re-matches the folder's recordings against the current library.
- **Read by the stacker and by tools** so nothing has to infer sequences or guess names. It also lists the order of recordings, so a burst series is explicit.
- **Checks**: each reference carries the file size so a replaced library file is noticed.
- **Portability**: copying a session folder alone leaves its references dangling. An export action copies the referenced calibration files next to the recordings (or into a `calibration/` subfolder) and rewrites the paths.
- **Seeing quality** (the per-frame sharpness summary of the seeing ranking idea) can live here per recording, so bursts can be sorted and chosen without opening the videos.

## What the app checks (the match key)

A dark matches a recording when camera, region size and position, bit depth, offset, gain and exposure agree, and the sensor temperature is within a tolerance (default 3 C). A flat matches on camera, region, bit depth, gain and the optical setup (dust and vignetting belong to the whole train) and, if the flat is a master, on its size. The Status overlay says which item differs ("dark: different offset") instead of only "mismatch".

One active dark and flat **per camera** (not per app), remembered across reconnects and restarts.

## Steps (each useful alone)

1. The match key and per-camera masters (prevents wrong calibration; no change to where files go).
2. Camera and setup tags, and the new folder and file names.
3. `catalog.json`: written, refreshed, read by the stacker.
4. The Files screen: filter and group by camera, setup and kind; "matching this camera" by default for calibration files; export with calibration.

## Open points

- Target at the top of a day's folder name is `saturn_533MC_...`; for the Sun, `sun_ha_...` (the profile).
- Tag collisions (two setups slugify to the same text): the app appends `-2` and shows the full name in the note.
- Region position changes between recordings in one folder: the catalog is per recording, so it handles that, but the file name's `roi` part names only the size.
