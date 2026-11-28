# 24. A self-describing file layout, a calibration library per camera and a catalog per folder

Date: 2026-11-28. Status: accepted; step 1 (the match key and per-camera masters) is built, the rest is not. The full text is the [proposal](../project/proposal-file-layout.md).

## Context

Recordings, darks and flats shared a day folder and nothing in a name said which camera or optical setup made a file. One master dark and one master flat were active for the whole app, and a dark was judged only by exposure and gain (and its pixel size). The data travels on a USB key to a laptop without RustFlare, where the folders and names must explain themselves, and where a tool must not have to guess which dark belongs to which video. Two cameras on one day, the 533 behind several trains, made all of this likely to go wrong.

## Decision

1. **A match key** is written into the note of every master dark and flat: camera, region (size and position), bit depth, offset, gain, exposure, sensor temperature and the optical setup. The app compares it with the camera's state and says what differs (a dark: camera, region, bit depth, gain, offset, exposure, temperature within 3 C; a flat: camera, region, bit depth, gain, optical setup). Built (`crates/server/src/calkey.rs`).
2. **One master dark and flat per camera**, remembered across restarts (config key `calibration`, by driver and id) and loaded when that camera opens; a camera never gets another's. A master from before this is taken over only when its note names this camera. Built.
3. **Layout**: `<date>/<target>_<camera tag>_<setup tag>/` for recordings (the Sun by its profile), `calibration/<camera>/darks|flats/` as a library reused across nights, `stacks/<date>/` with the same tags. Tags are short ASCII, a clash gets `-2`, file names carry the match key and the region's size only. Existing folders stay where they are and are still read. Not built.
4. **`catalog.json` in every recording folder**: the recordings in order with their settings, and for each the library dark and flat that calibrate it (relative paths, sizes, how good the match is), written after each recording, refreshed when new calibration frames appear, read by the stacker. An export copies the referenced calibration files next to the recordings. Not built.
5. The Files screen filters and groups by the same tags: a view over a layout that is already readable without the app.

## Consequences

- A wrong dark or flat is named in words instead of silently applied. Masters made before the key was written are judged by exposure and gain as before.
- Moving the USB key to a laptop gives readable folders and a catalog, not a pile of files.
- The calibration library is a second tree next to the day folders; tools that assumed darks and flats beside the recordings read the catalog instead.
