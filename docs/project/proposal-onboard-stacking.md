# Proposal: an on-board lucky-imaging stacker (`rustflare-stack`)

Status: proposal, nothing built. Research done by a delegated agent on 2026-11-22 from the owner's brief ("an AutoStakkert-type stacker on board, at low priority so it never disturbs a recording, a different engine and page, started by the sequencer on a directory and watching for more files"); edited and annotated by the maintainer's assistant.

!!! warning "Read the figures as estimates"
    The research made only a few web searches. Every per-frame time below is reasoned, not measured, crate names must be checked on crates.io before use, and the Debian version of Siril is a guess. Verify before committing to the plan.

## Recommendation

- Build a **pure-Rust engine**, a new workspace crate and binary `rustflare-stack` with its own systemd unit. Do not drive Siril or a Python sidecar (GPL, packaging weight, no use of our Sun model, awkward job control); keep them as reference tools for comparison.
- **v1 is modest:** per-burst quality ranking on the disc texture, alignment from the Sun model plus a phase-correlation refinement, a mean stack of the best 10 to 25 percent, 16-bit PNG and FITS plus a JPEG preview. **Multi-point local alignment** (what makes AutoStakkert good on the Sun, because seeing warps the disc) is milestone 3.
- **CPU is not the constraint**, **I/O on the shared FAT32 stick is.** Recordings and stacking share one USB stick (about 66 MB/s sustained write). The design reads a burst right after it is written (page cache), caps its own read rate (idle I/O class may be a no-op on the stick's scheduler), and **pauses while a capture runs** by polling `capture.state`.
- **Handoff by a spool directory** (a small job JSON the sequencer drops; survives restarts, needs no port, works after the plan is finished) plus a localhost HTTP API for the web page. The page is a new tab in the existing app, reached through the server's reverse proxy; it shows "stacker offline" when the unit is down.

## What the established tools do that matters here

AutoStakkert ranks frames (or alignment-point regions) by a gradient or Laplacian sharpness, finds a global shift against a reference, places alignment points on a grid (24 to 128 px) with their own per-frame shifts, keeps the best X percent globally or per point, and averages; drizzle is optional and sharpening is left to other tools. PlanetarySystemStacker follows the same pipeline (Python, GPL). Siril 1.5 added multi-point registration; Debian trixie probably ships an older Siril.

For solar H-alpha: quality is best measured on the **disc texture** inside a mask (not the limb); a **prominence lane** has the disc masked black, so align it on the limb and the Sun model, and keep more frames (about 50 percent) because the signal is faint; drizzle is of little value; sharpening is display-side only (the project keeps raw data as the product).

## Scope and expected performance of v1 (estimates)

Per burst of 100 frames of 1944x1472 16-bit (570 MB): read into RAM, rank all frames (about 1 s), align the kept 25 (global shift from the Sun model, phase correlation on a 1024 crop, about 80 ms each), mean-stack, write. Roughly **4 to 6 s of one core** per burst, against a 30 s cadence. With multi-point alignment (M3) about 8 to 12 s. The recorder writes about 19 MB/s on average for that cadence, so reading it once adds about as much; a read right after the write comes from the page cache.

## Architecture

- **Process:** `rustflare-stack.service`, `Nice=19`, `CPUWeight=10`, `CPUQuota=200%`, `MemoryMax=2G`, `IOWeight=10`, `IOReadBandwidthMax` for the stick, plus its own token-bucket read limit; restart on failure.
- **Queue:** a state file written atomically; on restart it resumes and skips bursts whose output exists.
- **Spool job:** `spool/<time>_<name>.job.json` with `{dir, watch, preset, until}`.
- **Watcher:** the `notify` crate plus a 30 s rescan (inotify can miss events on some USB filesystems). A SER counts as finished when its sidecar JSON exists, or its header frame count is non-zero and the size matches the layout, or it has not grown for 10 s. `name.ser` plus `name_part2.ser` are one burst. Per-frame FITS/PNG folders finish with their sidecar or when the file count stops growing.
- **Outputs:** a `stacks/` folder next to the session: `stack_<burst>.png`, `.fits`, `.jpg` and a `manifest.json` (sources, preset, frames kept, quality numbers).
- **Pause policy (owner selectable, default "gaps only"):** pause while recording; or process between bursts and pause the moment one starts; or always run under the read cap. Back off for 60 s after a stall.
- **Web page:** queue and progress, thumbnails of finished stacks, presets (percent kept, alignment mode, point size), a time-lapse strip and download.

## What the app must do for it (not checked by the research)

The sidecar JSON of a burst does **not** hold the per-frame Sun centre and radius today (it has name, burst, frames, duration, exposure, gain). The recorder should write them (one line per frame, or a small side file) so the stacker does not have to re-estimate them. The sequencer needs a "stack this" option that drops the spool job. The partial-file handling added after the unsafe-removal test (header frame counts every 50 frames, `scripts/ser-repair.py`) already helps the watcher recognise cut files.

## Testing without hardware

Synthetic fixtures generated in the tests (a textured disc with a limb, known sub-pixel shifts, known blur, noise, and a smooth local warp as "seeing"): the recovered shift within 0.1 px (global) and 0.3 px (multi-point), the sharpest frames ranked first, and "best 25 percent beats the full mean" as a hard test. Watcher tests with temp directories (a SER written in chunks is not processed early; `_part2` merges; kill and resume without duplicates). One real SER kept out of the repository, compared against PSS or AutoStakkert, with only the numbers kept.

## Staged plan (estimates)

| Milestone | Shippable | Effort |
|---|---|---|
| M1 | headless: watcher, ranking, global align (Sun model plus phase correlation), mean stack, PNG/FITS/JPEG, spool, state file, systemd unit | 2 to 3 weeks |
| M2 | web page, presets, HTTP API, pause policy | 1 to 2 weeks |
| M3 | multi-point local alignment with local quality selection | 3 to 4 weeks |
| M4 | sigma clip, drizzle, sharpening, time-lapse assembly | 2 to 3 weeks |
| M5 | colour (OSC) path | 1 to 2 weeks |

## Risks and questions for the owner

I/O contention on the stick is the main risk and needs a measurement on the Pi (a 4-hour test with a recording running). Quality will trail AutoStakkert and PSS until M3 and probably after. Questions: is "gaps only" the right default; should the recorder write the spool job (a small sequencer change) or should the stacker discover directories; is the disc texture or the limb the priority; should stacks be saved unsharpened only.

## Sources

[AutoStakkert](https://www.autostakkert.com/wp/?p=7), [PlanetarySystemStacker](https://github.com/webosfredo/PlanetarySystemStacker), [Siril multi-point](https://siril.readthedocs.io/en/latest/preprocessing/multipoint.html), [notify](https://docs.rs/crate/notify/latest/source/src/inotify.rs), [inotify](https://docs.rs/inotify).
