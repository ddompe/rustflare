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
