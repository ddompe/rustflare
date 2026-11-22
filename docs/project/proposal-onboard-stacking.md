# Proposal: an on-board lucky-imaging stacker (`rustflare-stack`)

Status: **M0 to M3 built** (command-line stacker, multi-point alignment, Pi service, web page; see the review sections below). M4 (sharpening, time-lapse, colour, drizzle) and the open checks are listed at the end. Research done by a delegated agent on 2026-11-22 from the owner's brief ("an AutoStakkert-type stacker on board, at low priority so it never disturbs a recording, a different engine and page, started by the sequencer on a directory and watching for more files"); edited and annotated by the maintainer's assistant.

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

## Review of the plan (2026-11-22) and what changed

The first version of this plan was checked against the repository and corrected:

- **"Finished" detection.** The burst sidecar is written when the burst ends, *before* the writer thread has put the frames on disk, so a sidecar is not a "file complete" signal. The engine should write a `<name>.ready` marker once the writer has closed the file; for folders from other sources the SER layout check (header, trailer, size) is the fallback.
- **Handoff.** The marker replaces the separate spool directory. No HTTP API, extra port or proxy in v1: the stacker writes a status file the page reads.
- **Per-frame Sun position** is not needed: the stacker aligns frames itself (phase correlation) and can reuse the `rustflare-tracking` crate if it ever needs the limb. One centre and radius per burst in the sidecar would be cheap.
- **Dark and flat correction** had been left out. The stacker applies the master flat (and a dark when its exposure matches) to every frame.
- **Streaming and lanes:** long bursts must not be held in RAM (the stacker reads frame by frame, caches the binned frames only when they fit), and each lane is stacked on its own.
- **Order:** a command-line tool first, tested on real recordings on the Mac, before any service or page.

Decisions by the owner: **prominence lanes first, FITS stills as the product, Mac first and the Pi when it proves worth it.**

## M0 results (2026-11-22)

`crates/stack`, `rustflare-stack stack <file.ser> [--flat master.png] [--dark master.png] [--keep 25] ...`: reads SER (also `_partN`, files cut short), applies flat and dark, aligns every frame by phase correlation on 2x-binned, high-passed images (smooth window, so a coronagraph mask's edge does not pull shifts to zero), ranks the frames, averages the best 25 % and writes a FITS still. Tested on synthetic bursts with known shifts, blur and noise (`crates/stack/tests`) and on a real prominence-lane burst and its disc-lane partner from the 2026-11-22 field day (100 frames of 1944x1472, 572 MB each; the recordings stay out of the repository).

- **Speed (measured, Mac, 4 threads, files in the page cache):** a 100-frame burst takes 1 to 2 s to read, align and rank and 0.1 to 0.3 s to stack. A Pi 5 core is several times slower, so expect roughly 5 to 15 s per burst on two threads (an estimate, to be measured), well inside a 30 s cadence.
- **Ranking by one frame's Laplacian is mostly noise.** The chromospheric texture is low contrast, so the default score compares each aligned frame's high-pass detail with the burst's own mean. Measured by the detail two independent half-stacks have in common (noise cancels), on both lanes: reference score > Laplacian > file order > worst frames, and keeping fewer frames is better (prominence lane: best 10 % 0.00089, best 25 % 0.00077, all 0.00048, worst 25 % 0.00031).
- **Alignment is real and helps the disc lane**, whose frames wander by up to about 5 px in a burst (checked independently with numpy): the aligned stack shows visibly crisper fibrils than the unaligned one. **The split-half number does not show it** (it favours the unaligned stack, probably because sensor-fixed pattern correlates between halves when frames are not moved), so use it only to compare ranking methods with alignment on, and judge alignment by eye. A better fixed-pattern-immune metric is an open item.
- **Prominence lane:** the shifts in that burst were about 0.1 px, so alignment changes little there; the stack is much cleaner than a single frame.
- **Known gaps:** integer or bilinear shifts only (no Lanczos/drizzle), one global shift per frame (no multi-point alignment yet: the limb and prominences will show seeing distortion), no sigma clipping, colour (mosaic) recordings refused, no dark scaling for a different exposure (the prominence lane's 59 ms exposure has no matching dark), no watcher, no service.

Next: check the stacks on more bursts of the day, then M1 (the `.ready` marker, watcher, queue, resource limits).

### Raw in, float out (owner's rule, 2026-11-22)

The stacker reads **only raw data**: SER recordings, and dark and flat masters it builds itself from the raw dark and flat SER files (mean in float, flat minus dark, normalised). It does not read the recorder's `_master.png` files, which are derived, rounded convenience copies for the live view. To make that possible the recorder now keeps the **raw dark frames** as a SER next to the master (flats already did). The output is a **32-bit float FITS** by default (`--u16` for unsigned 16-bit).

## What the documentation of the established stackers says (read 2026-11-22)

Sources: the [Siril multi-point documentation](https://siril.readthedocs.io/en/latest/preprocessing/multipoint.html), the [PlanetarySystemStacker README](https://github.com/deix/PlanetarySystemStacker), and the [AutoStakkert site](https://www.autostakkert.com/wp/). The AutoStakkert pages do not document the algorithm (only change notes and download links; Windows only, version 4.0.13, development "slowed to a crawl"), so what follows for it is the common pipeline, not its internals.

| Step | Siril 1.5 multi-point | PlanetarySystemStacker | RustFlare M0 |
|---|---|---|---|
| Quality | Laplacian, brightness-normalised | overall quality, then local contrast per point | agreement of the aligned frame's detail with the burst mean |
| Reference | average of the best 5 % of frames | patch with most structure on the best frame, then a mean of the best | the single best frame |
| Global alignment | "Surface (detail)" mode for solar and lunar | on the structure patch | phase correlation on high-passed, windowed images |
| Local alignment | grid of correlation boxes (half-width 24, pitch 2.25x), min brightness, contrast and structure filters; search width 14 px; shifts smoothed (radius 2.5); optional refinement against a first-pass stack | mesh of alignment points, dim and flat ones dropped | none yet |
| Frame selection | **per point**: "register percent" and "stack percent", with a soft raised-cosine cutoff | **per point**, best N | one set for the whole image |
| Stacking | sub-pixel patch shifts, Hann-feathered blending, brightness matching; "object changing fast" for the Sun | stack the patches, blend, fill gaps from the global background | global shift, mean |
| Output | 16-bit integer FITS by default, optional float | not stated | 32-bit float FITS |
| Sharpening | not part of stacking | optional, separate step | none (display side) |

What this means for us: global alignment plus a global frame set (M0) is only the first half. For the Sun, **local shifts and per-region frame selection are what both tools rely on**, because seeing warps the disc differently in different places and the best frames for one patch are not the best for another. Two cheap steps before that: build the reference from the mean of the best 5 to 10 % and iterate (refine against a first-pass stack), and normalise brightness per frame.

## Revised milestones

| | Milestone | Gate |
|---|---|---|
| M0 | **Done.** CLI, raw in, float FITS out, global alignment, reference-agreement ranking, flat/dark from raw SER | synthetic tests; looks right on a real burst |
| M0.5 | Reference from the best 5 to 10 % and refine against a first-pass stack; per-frame brightness normalisation; robust mean (sigma clip); a fixed-pattern-immune quality metric; a batch command for a whole session folder with FITS headers from the sidecar (exposure, gain) and a manifest | several bursts across the hour; numbers stable |
| M1 | **Multi-point alignment** in pure Rust: grid, per-point search, smoothed shifts, per-point selection with a soft cutoff, feathered blend | visibly better than M0 on disc and limb; compare with Siril 1.5 on the same bursts |
| M2 | Pi: `.ready` marker from the recorder, watcher, queue, systemd limits, benchmark on the Pi with a recording running | the owner decides the Pi step is worth it (after M1) |
| M3 | Web page (status file, thumbnails, presets) | |
| M4 | Sharpening (display only), time-lapse of the stacks, drizzle (low value unless undersampled), colour | |

Multi-point moves **ahead** of the service: it is what decides whether on-board stacking is worth having at all.

## Follow-up from the raw-data rule

The recorder still writes its dark and flat masters as 16-bit PNG and applies those to the live view. They are display copies, but they are also rounded means; writing them as float FITS (and reading those back) would be consistent with the rest. Not done yet.

## M0.5 done and review (2026-11-22)

Built: reference refined against the aligned mean of the best 10 % (second alignment pass), per-frame brightness normalisation, optional sigma clipping (`--clip`), a `session` command that stacks every recording under a folder into a mirrored `stacks/` tree with a manifest (finished ones are skipped, so it can be run again as files arrive), and darks chosen per recording by exposure and gain from the sidecars (a dark that does not match is *not* applied, and the manifest says so). Tests: clipping removes an injected hot pixel, brightness changes are evened out, the second pass converges. On the real disc burst the second pass moved frames by 0.37 px (so the first pass had nearly converged); clipping removed about 1 % of pixel values.

**Assumptions checked**

- *"Alignment quality needs a new metric."* Dropped. The split-half measure is valid when every compared stack is aligned (the fixed pattern is spread out in all of them), so it can compare ranking methods and **global against multi-point alignment**; alignment against no alignment is judged by eye and by the convergence number (`refine_change_px`).
- *"A single global shift per frame is enough."* Not for the Sun. At 0.78 arcsec per pixel the isoplanatic patch is of the order of 5 to 10 arcsec (6 to 13 px) in daytime seeing, so patches 100 px apart move independently. This is the case for multi-point (M1), and it is also what the Siril and PSS documentation build on.
- *"Sigma clipping by default."* No: it triples the reads of the kept frames and removed only 1 % here; it stays opt-in.
- *Not yet true:* no real flat or raw dark from the field day exists (the flat SER was not on the stick and darks were never kept raw before today), so calibration is tested on synthetic data only. The first raw flat and dark taken with this release will be the real test.

**Plan correction for M1:** use the refined mean as the AP reference; correlate on 2x-binned images (the cache already holds them; half-width 24 binned pixels, pitch 2.25x like Siril); register only the better half of the frames per point (Siril's "register percent") to bound the cost; rank per point by the normalised correlation peak; blend with a Hann window straight into one accumulator (no per-point buffers); fall back to the global stack where no point has enough structure. Success is measured by the split-half detail of the multi-point stack against the global one on the same bursts, and by eye on the limb and the granulation.

## M1 done and review (2026-11-22): multi-point alignment

Built (`crates/stack/src/multipoint.rs`, wired into `stack.rs`; on by default, `--global` turns it off): a grid of alignment points on the structured parts of the reference (the aligned mean of the best frames), a 2x-binned local correlation per point and frame (box half-width 24, search 14 px, pitch 1.5x half-width), registration of the better half of the frames only, the best 25 % chosen **per point** by normalised correlation, Hann-window blending straight into one accumulator, and the global stack showing through where no point reaches. `--clip` uses the global stack (it turns multi-point off).

**Results on the field-day bursts** (split-half detail, both stacks aligned so the comparison is valid): disc lane 0.00035 against 0.00025 for global alignment (+40 %, 322 points), prominence lane 0.00099 against 0.00084 (+18 %, 111 points, glow around the prominence correctly left out). By eye the multi-point disc stack is visibly crisper (finer fibrils, more contrast) with no seams; the prominence base and limb ring are crisper too. Local shifts differ from the global shift by 1.3 to 1.7 px rms, which is the distortion one shift per frame cannot follow. Synthetic tests: the local aligner recovers a known displacement field to under 0.5 px, and on frames bent by a smooth field multi-point beats global by 12 % on the same measure (the real data gains more than the synthetic scene, whose texture is smooth).

**Cost (measured on a fast Mac core):** 100 frames of 1944x1472, global plus multi-point, 7 to 8.5 s on one thread and about 4 s on two. A Pi 5 core is several times slower: expect **12 to 25 s per burst on two cores**. That fits a 30 s cadence only just, and only if the stacker really gets two cores. Levers if needed: register fewer frames (`--register 30`), a larger point pitch, fewer frames kept, or stacking only every Nth burst.

**Assumptions checked or revised**

- *Multi-point is what matters for the Sun.* Confirmed by the numbers and the pictures above.
- *The error against the true scene can judge a stack.* No: every stack sits in the distorted coordinates of its reference frames. The split-half detail is the measure (and it needs both stacks aligned).
- *Per-point reference from the first-pass mean.* Works; the mean is a little blurred by the frame-to-frame distortion, which costs some correlation sharpness but not bias.
- *Not compared with Siril 1.5 or AutoStakkert* (not installed here, and AutoStakkert is Windows only). That comparison stays open; it would be worth doing with the owner's own tools on the same SER.
- *Still untested on the Pi,* with real flats and darks, and with a recording running at the same time.

**Plan correction for M2 (the Pi service):**

- The pause policy does not poll HTTP: the recorder writes `capture.state` (state, storage folder, time) to a small file under `/var/lib/rustflare/` on every change and every 10 s, and the stacker reads it (stale means the recorder is not running, so nothing is recording). The stacker also learns the current storage folder from it.
- The recorder writes `<recording>.ready` when the writer has closed the last part of a burst, which is the "file complete" signal (the sidecar is not).
- The stacker is a subcommand, `rustflare-stack watch`, run by a systemd unit that is **off by default**, with `Nice=19`, `CPUWeight`, `IOWeight`, `IOSchedulingClass=idle`, `MemoryMax`, and its own read-rate cap; its settings (enabled, keep percent, flat and dark paths, pause policy) live in `/var/lib/rustflare/stack.json`, which the web server will edit in M3.
- Stacking a burst costs up to 25 s of two cores on the Pi: default policy "only between bursts", threads 2.

## M2 done and review (2026-11-22): the Pi service

Built: the recorder writes a `<recording>.ready` marker once the writer has finalised and flushed the last part of a burst (sidecars are written earlier, so they are no signal), a `capture.state` file (`busy`, `next_in_s`, storage folder, refreshed every 10 s), and the file manager hides the markers. `rustflare-stack watch` reads those, keeps its settings in `stack.json` and its status in `stack-status.json`, and stacks what is finished, under a pause policy (`when_idle`, `between_bursts` with a minimum gap, `always`) that never starts while frames are being written. `rustflare-stack.service` (off by default; nice 19, CPUWeight 10, CPUQuota 200 %, idle IO class, 2 GB memory ceiling) ships in the package with the binary, and `rustflare-user` keeps its account in step with the recorder's. `scripts/sim/stacker.sh` runs the recorder and the stacker together on the simulators: markers, state file, paused during a recording, everything stacked once idle, nothing redone on a second run.

**Measured on the Pi 5** (real field-day bursts from the stick, output on the Pi's own disk, no throttling, 61 C): 100 frames of 1944x1472 take **7 to 8 s on two cores** (5.6 s on four, 11.9 s on one), cold or warm cache. The first Pi run took 19 to 20 s; profiling showed two global alignment passes at 3 s each, so with multi-point on the global shift is now found on 4x4-binned images and not refined (the points correct it), which halved the time on the Mac and cut it by more than half on the Pi, at a cost of about 5 % on the prominence lane's detail measure and none on the disc lane's.

**Assumptions checked or revised**

- *Poll the recorder's HTTP status.* Replaced by the state file: no network, no auth, survives the server being down.
- *"Between bursts" is safe.* Only partly: a stack started in a gap is not interruptible and can run into the next burst (7 s against gaps of 15 s or more is fine; a 5 s gap is refused). At nice 19 with idle IO it should still be harmless, but that is exactly what the hours-long test with a real recording must show (checklist item, and #86's stall counters). The conservative `when_idle` is one setting away.
- *Reads of the same stick compete with writes.* Not measured under load yet. The Pi run above read the stick cold (cache dropped) with nothing else running.
- *The first deploy of this package prompted about `/etc/default/rustflare`* (the default file lost its updater settings): upgrading with an edited file asks keep-or-replace; answer keep. `pi-deploy.sh` now passes `--force-confold`.
- *Not done by design in M2:* a web page and API (M3), colour, a queue priority across sessions, stacking from other folders than the recorder's.

**Plan correction for M3 (web page):** the server reads `stack-status.json` and the stacks' `manifest.json` (no new port, no proxy), lists the stacks (thumbnails made on demand from the FITS, small JPEG via a new endpoint), and writes `stack.json` through a small settings API; the page is a new tab-level section under More (not a sixth tab: stacking is a background service, not part of the observing flow), with: state and progress, a thumbnail grid of finished stacks with the numbers (frames kept, points, signal), the policy and keep-percent settings, and a link to download the FITS.

## M3 done and final review (2026-11-22): the page

Built: `GET /api/stack` (service state from `systemctl is-active`, `stack-status.json`, the settings, the newest 48 stacks from `stacks/manifest.json`, and the raw dark and flat recordings on the drive), `PUT /api/stack/config` (validated and clamped, atomic, flat and dark paths must stay inside the storage folder), `GET /api/stack/thumb/<path>` (a PNG preview made on first request, cached in `stacks/.thumbs/`), and **More > Stacking**: status, a grid of stacks with thumbnails that download the FITS, and a collapsed *Settings* section with the same options as `stack.json` including a flat chooser and a dark checklist. Playwright test (phone and desktop): records on the simulator, runs the stacker the way the service would, checks the thumbnail loads, the settings round trip, bad values are refused and clamped. CI now builds the stacker for the browser tests.

**Assumptions checked or revised**

- *A page cannot start the service.* Right: the server runs unprivileged, so the page reports the service state and shows `sudo systemctl enable --now rustflare-stack`. The *enabled* switch pauses stacking without touching systemd.
- *A sixth tab.* No: it is a section under More, because stacking is a background service and not part of the observing flow.
- *Thumbnails as JPEG.* PNG instead (an 8-bit grey PNG is small enough, and the encoder was already in the build); they are display copies only, the FITS stays the product.
- *The server depends on the stacker library.* Yes (for the FITS reader and the preview); it adds `rustfft` to the server build, a pure-Rust dependency.

**Where this leaves the plan**

Done: M0 to M3. Left, in this order:

1. **Real-world checks (need the owner, the Pi and a sunny day):** the hours-long run alongside a real recording (writes not slowed, no capture stalls, #86), a real flat and dark, and the comparison with AutoStakkert or Siril on the same SER. Until those pass, the stacker should be described as *tested on field-day recordings, not yet in routine use*.
2. **Quality work:** colour (mosaic) recordings; Lanczos resampling instead of bilinear (a sharper stack at the same cost); frame-selection weights instead of a hard cut (Siril's soft raised-cosine); drift of the reference over a long burst; a way to stack a *series* (all bursts of an hour) into one deeper image.
3. **M4 products:** a time-lapse of the stacks (needs a video encoder; probably an `ffmpeg` call if installed, otherwise a folder of PNGs), optional display-side sharpening (wavelets or deconvolution, never in the FITS), drizzle for undersampled setups (low value for the LS50 and Apollo-M).
4. **Housekeeping:** disk quota for `stacks/`, an option to delete stacks older than N days, and the stacker's logs into the debug API.
