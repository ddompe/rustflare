# Stacking

RustFlare can stack your recordings into sharper still images, on the Pi, without disturbing a recording. It is a separate program, `rustflare-stack`, that works on the raw SER files and writes FITS stills; the recordings themselves are never changed.

!!! note "Status"
    Tested on the simulators and on real field-day recordings (disc and prominence lanes, on a Pi 5 and a Mac). Not yet run for hours alongside a real recording, and not yet compared with AutoStakkert or Siril on the same data.

## What it does

For each recording (a burst of frames), it applies your dark and flat, aligns every frame, keeps the best ones (25 % by default), averages them, and writes a 32-bit float FITS file. The alignment is **multi-point**: besides one shift per frame it measures a shift for many small patches, because seeing bends the picture differently a few arcseconds apart, and each patch keeps its own best frames. On a field-day disc burst this gave visibly crisper detail than a single shift per frame. A 100-frame burst of 1944x1472 takes about 8 seconds on two cores of a Pi 5.

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

## Running it on the Pi as it records

```sh
sudo systemctl enable --now rustflare-stack
```

The service is **off until you enable it**, runs at the lowest CPU and disk priority with a CPU and memory ceiling, and as the same account as the recorder. The recorder writes a small `capture.state` file and a `.ready` marker beside each finished recording; the stacker reads those and stacks every finished recording, oldest first. Results go to `stacks/` on the same drive (the folder structure is mirrored, with a `manifest.json`).

Settings are in `/var/lib/rustflare/stack.json` (all optional):

| Key | Default | Meaning |
|---|---|---|
| `policy` | `between_bursts` | `when_idle`: only when nothing is recording. `between_bursts`: also in the gaps of a recording that waits for its next burst (when the gap is long enough, see `min_gap_s`). `always`: whenever there is work. It never starts while frames are being written, except with `always`. |
| `min_gap_s` | 15 | The shortest gap in which `between_bursts` starts a stack. |
| `keep_percent` | 25 | Frames kept, the best first. |
| `multipoint` | true | Alignment points; false is faster and a little less sharp. |
| `threads` | 2 | |
| `flat`, `darks` | none | Raw SER recordings, absolute or relative to the storage folder. |
| `enabled` | true | |

Progress is in `/var/lib/rustflare/stack-status.json` (state, what it is working on, how many are waiting). If the Pi is turned off, the stacker carries on with whatever is not stacked yet the next time it runs.

## What to expect

- Stacks are **derived data**: the SER files stay the product. Delete `stacks/` any time to start again.
- The stack is as sharp as the seeing allows. It does not sharpen; apply wavelets or deconvolution in your own tool.
- Colour recordings are not supported yet.
