# Stacking

RustFlare can stack your recordings into sharper still images, on the Pi, without disturbing a recording. It is a separate program, `rustflare-stack`, that works on the raw SER files and writes FITS stills; the recordings themselves are never changed.

!!! note "Status"
    Tested on the simulators and on real field-day recordings (disc and prominence lanes, on a Pi 5 and a Mac). Not yet run for hours alongside a real recording, and not yet compared with AutoStakkert or Siril on the same data.

See also [After each burst](recording-modes.md): the recorder can stack bursts straight from memory and skip the video.

## What it does

For each recording (a burst of frames), it applies your dark and flat, aligns every frame, keeps the best ones (25 % by default), averages them, and writes a 32-bit float FITS file. The alignment is **multi-point**: besides one shift per frame it measures a shift for many small patches, because seeing bends the picture differently a few arcseconds apart, and each patch keeps its own best frames. On a field-day disc burst this gave visibly crisper detail than a single shift per frame. A 100-frame burst of 1944x1472 takes about 8 seconds on two cores of a Pi 5.

## The page: More > Stacking

More > Stacking shows what the stacker is doing (stacking a recording, paused and why, nothing to do), how many stacks there are with an **Open the stacks in Files** button (the stacks themselves, with thumbnails, previews and downloads, are looked at in the [file manager](record.md#files-file-manager), in the `stacks` folder), and, under *Stacker settings*, the same options as `stack.json`: on/off, when to stack, frames kept, multi-point, the black disc of prominence stacks, the flat, and which darks to use. The main ones (on/off, multi-point, frames kept, the black disc and its size) are also on the Record tab, under *After each burst > Stacker settings* (the options are on the Record tab as well as in More), so they can be set while you set up a recording (it lists the raw flat and dark recordings on the drive, with exposure and gain). If the stacker service is not running (unusual: it is enabled with the package), RustFlare starts it again by itself, after 20 s and then with growing pauses, and the page says how many times it tried and why a try failed (the package installs a polkit rule, for the account the recorder runs as, that lets it start that one service). Until it is back, bursts are saved as video, and the Start button says so.

## Prominence stacks and the black disc

A prominence lane recorded with **Save the disc position with every prominence frame** keeps its frames raw: the Sun's disc is saturated, not blacked out. The recorder saves where its Sun model put the disc in each frame. After stacking, the stacker moves those positions by each kept frame's alignment, takes the median, fits a circle to the disc's edge in the stack itself, and draws a black disc of 100 % of that radius (a recorded position, used when the edge cannot be found, gets at least 103 % because it is only known to a few tens of pixels). A real prominence recording showed why the margin is small: at 103 % most of a prominence's base was hidden, at 101 % it was complete, and 100 % gave the best result by eye. Doing it once on the stack, instead of frame by frame, removes the jumping disc and the rings it left at the limb. If the edge cannot be found (a cloud, the Sun out of view) the recorded position is used and the manifest says so.

In More > Stacking, **Black disc on prominence stacks** switches the drawing off (the stack then keeps the disc as recorded, with its position in the FITS header as `DISCX`, `DISCY`, `DISCR`) and sets its size. The setting applies to stacks made from now on. To change an existing stack, open it in **Files** and press **Re-stack**: the stack is deleted and made again from its recording with the current settings. Prominence recordings made before 0.6 have the disc blacked out in the recording itself, so re-stacking them cannot undo it.

## Running it by hand

```sh
rustflare-stack stack burst.ser                       # one recording, FITS in stacks/ next to it
rustflare-stack session /media/pi/DRIVE/rustflare/2026-11-22/prominence_...   # every recording under a folder, into stacks/
```

With a flat and darks (raw recordings made in the Camera tab; the stacker builds its own masters from the SER files and never reads the PNG copies):

```sh
rustflare-stack session <folder> --flat flat_143838Z.ser --dark dark_142435Z.ser --dark dark_142433Z.ser
```

A dark is applied only if its exposure (within 3 %) and gain match the recording's; otherwise it is left out and the manifest says so. Other options: `--keep 10` (percent of frames), `--global` (one shift per frame, faster), `--clip 2.5` (sigma clipping; uses global alignment), `--u16` (16-bit FITS instead of float), `--threads N`. `rustflare-stack` without arguments lists them all.

## Running on the Pi as it records

Nothing to start: the package installs the stacker as a service and **enables it**, so it is always ready. It runs at the lowest CPU and disk priority with a CPU and memory ceiling, as the same account as the recorder, and starts again by itself after a reboot or a crash. (To switch it off, use the switch in More > Stacking; `sudo systemctl disable --now rustflare-stack` stops the service itself.)

It only stacks **recordings made after it was installed**, the ones the recorder has marked finished. Older recordings on the drive are left alone unless you turn on *Also stack older recordings* in More > Stacking (or `"include_unmarked": true` in `stack.json`), or run `rustflare-stack session <folder>` by hand. The recorder writes a small `capture.state` file and a `.ready` marker beside each finished recording; the stacker reads those and stacks every finished recording, oldest first. Results go to `stacks/` on the same drive (the folder structure is mirrored, with a `manifest.json`).

Settings are in `/var/lib/rustflare/stack.json` (all optional):

| Key | Default | Meaning |
|---|---|---|
| `policy` | `between_bursts` | `when_idle`: only when nothing is recording. `between_bursts`: also in the gaps of a recording that waits for its next burst (when the gap is long enough, see `min_gap_s`). `always`: whenever there is work. It never starts while frames are being written, except with `always`. |
| `min_gap_s` | 15 | The shortest gap in which `between_bursts` starts a stack. |
| `keep_percent` | 25 | Frames kept, the best first. |
| `multipoint` | true | Alignment points; false is faster and a little less sharp. |
| `threads` | 2 | |
| `flat`, `darks` | none | Raw SER recordings, absolute or relative to the storage folder. |
| `include_unmarked` | false | Also stack older recordings that have no finished-marker. |
| `telemetry` | true | Keep the behaviour log described above. |
| `enabled` | true | |

Progress is in `/var/lib/rustflare/stack-status.json` (state, what it is working on, how many are waiting). If the Pi is turned off, the stacker carries on with whatever is not stacked yet the next time it runs.

## Telemetry: how it behaves next to a recording

The stacker keeps a record of its own behaviour so you can review later how it coexisted with a real recording. It is on by default (`"telemetry": false` in `stack.json` turns it off) and written as JSON lines to `/var/lib/rustflare/logs/<date>/stack.jsonl`, next to the recorder's logs (same size and age limits; the debug API serves them: *More > Session log* and [Debugging](../reference/debugging.md)):

- **start:** the limits it was given (cgroup `cpu.max`, `cpu.weight`, memory limit, the cores it may run on, nice value, systemd's CPU quota, I/O class and weight), and the config.
- **state:** every change between idle, working and paused, with the reason and what the recorder was doing at that moment.
- **job:** per recording: wall time, CPU seconds (user and system) and how many cores that is on average, CPU throttling by its quota (periods and seconds), busy percentage of every core, memory and I/O pressure stalls, the time in each phase, and a summary of 2-second samples during the job (temperature, clock, firmware throttle flags, memory, which cores its threads ran on, and whether the recorder was writing frames).
- **irq / cores:** how many USB, SD-card and network interrupts fired on each core during the job, next to the busy percentage of each core, so a stacker job on a core that also serves the camera's USB interrupts would show.
- **sample** every 2 s during a job, **idle** once a minute between jobs.

`scripts/analysis/stack_telemetry.py /var/lib/rustflare/logs` (or `http://<pi>:8080`) prints the limits, a table of jobs, the pauses and anything that looks like interference (throttling, high temperature, an I/O stall, a job that ran while the recorder was writing). A first run is in [the telemetry run of 2026-11-22](../project/stacker-telemetry-run-2026-11-22.md).

## What to expect

- Stacks are **derived data**: the SER files stay the product. Delete `stacks/` any time to start again.
- The stack is as sharp as the seeing allows. It does not sharpen; apply wavelets or deconvolution in your own tool.
- Colour recordings are not supported yet.
