# Record tab

![Record tab](../img/manual/record.png)

Frames go into a RAM buffer and are written to disk in the background, so the camera does not slow down for the disk until the buffer is full.

## What to record

- **Single frame**: one still.
- **Sequence**: stop by frame count and/or duration.
- **Bursts**: for example "100 frames every 30 s". Set frames or seconds per burst, the spacing (start to start) and how many bursts (blank means until you press Stop). Live view and guiding keep running between bursts.
- **Prominence video**: raw video in two lanes, the disc at your current exposure and a longer exposure (a multiple you choose) for the prominences, alternating. Nothing is combined or stretched; every lane and cycle is its own short SER video in a `prominence_...` session folder, ready for lucky-imaging stacks that become the frames of a time-lapse. (Prominence pair, below, takes single PNG stills with a ready-made composite.)
- **HDR bracketing lanes**: for eclipses and the Moon. Define 2 to 6 lanes, each with a name, exposure, optional gain and a frame count or duration. They are cycled in order for the number of cycles you give, and "Start a new cycle every" sets a steady time-lapse cadence. Every lane and cycle is its own short raw video in an `hdr_...` session folder, one sub-folder per lane. Stack each video into one frame; the stacks of a lane, in cycle order, make a time-lapse, and the lanes merge into HDR frames. Your exposure and gain are restored at the end. The file manager shows these sessions as their own kinds, with a per-lane list and a filter.

## Prominence pair

Saves a disc frame, a longer exposure for the prominences and their composite. Judge the exposure first with the [Prominence view](screen.md#prominence-view).

## Format

- **SER** video (default), **PNG** or **FITS**. PNG and FITS write one file per frame into a folder. FITS headers carry time, exposure, gain and instrument.
- *When the RAM buffer is full*: **Wait for the disk** never loses a frame (a fast camera slows to disk speed). **Drop frames** keeps the camera at full speed and counts the dropped frames, shown in the buffer overlay and the notes file.

The buffer size (default 1 GB) is set here. The panel shows whether a burst fits and the disk load of the schedule. File layout and sizes: [Capture formats and storage](../reference/capture-formats.md).

## Flats and darks

These are captured from the [Camera tab](camera.md#flats). Recordings always stay raw.

## Captures (file manager)

**Captures** lists what you recorded, by day.

- **Check** reads a recording's header and samples frames: size, frame count, measured fps, duration and brightness. It flags **Mostly dark** (cap left on, or the Sun out of view) and **Incomplete** (cut off).
- **Download** and **Delete** work per capture. Delete asks for confirmation and removes the notes file too.
- You can check or delete several at once.

## Safe eject

Use **Eject drive safely** (More > Storage) before pulling a USB drive. It is refused while recording.
