# Design: recording modes, their UI and their manuals

Status: plan (2026-11-22). Built so far: the shared frame ring (`rustflare-ring`) and [ADR 0019](../adr/0019-shared-frame-ring.md). Nothing of the UI exists yet; each piece below ships with the stage that makes it true, so the page never offers something the engine cannot do.

## The scenarios

| Mode | What is kept | Typical use | Disk at 100 frames of 1944x1472 every 30 s |
|---|---|---|---|
| **1 Keep video** | every frame as SER (today) | anything you will process yourself | about 68 GB/h |
| **2 Keep video and stack** | every frame, plus a stack made from memory | eclipses, first sessions with a new setup | about 68 GB/h plus 1.3 GB/h |
| **3 Stack only** | only the stack (FITS), the frames only if the stack fails | the everyday solar session | about 1.3 GB/h |
| **4 Best frames only** | the stack plus only the best N % of the frames as SER | stack now, keep the option of re-stacking | about 17 GB/h at 25 % |

Settings: **frames used in the stack (%)**, default 25; **frames kept as video (%)** for mode 4, default 25 (it may differ from the stack's); **safety margin of the stack-time check**, default 2x (how many times the measured stack time must fit into the time a burst has left before its frames are overwritten; 1 to 5); multi-point alignment; flat and darks (already in More > Stacking). Always on, not a setting: **a burst the stacker cannot finish before its frames are overwritten is saved as video instead**, so capture never suffers and a stack-only burst is not silently lost.

## Errors are never quiet

Any failure in this pipeline is shown, recorded and queryable; nothing falls back, drops, retries or gives up silently. Every error has the same five outputs:

1. **On the page:** a banner in the Record tab (and the status bar when another tab is open) that stays until it is acknowledged, saying what happened, which burst, and what the system did about it ("Burst 7: the stack could not finish before its frames were overwritten; the video was saved instead").
2. **In the log:** an event in `events.jsonl` with the burst, the numbers and the cause.
3. **In the API:** `status.errors` (a list with an id, time, kind, burst, message and whether it has been acknowledged) and counters per kind, so a script or the stacker's telemetry can see them.
4. **On the burst:** the burst record (and the stack's manifest entry and FITS header) carries a flag and the reason, so a stack that came from a degraded burst says so.
5. **In the stacker's telemetry** where it concerns the stacker.

What counts as an error (each its own kind): a burst falling back to video (any reason: the stack-time check failed, the stacker is not running, the stacker died, a stack job found its frames overwritten, memory short); a stack that failed or looks wrong (no valid points, shifts far outside the range, fewer frames than asked); frames dropped or the camera slowed because the buffer was full; a frame gap over the threshold; the writer failing or the drive filling or disappearing; the ring's producer or a consumer dying or being lapped; a consumer that could not attach; mode 3 or 4 requested but the stacker unavailable at the start (refused with a message, not started silently as mode 1); the stacker's own errors; a flush that did not complete. A fallback that *worked* (video saved instead) is still shown, as a warning rather than an error.


**Record tab, a new section "After each burst"** (for bursts and lanes; a single sequence offers modes 1 and 2):
- one control with the four modes, each with a one-line description and **the disk it will use per hour** for the current plan, computed live from the frame size, burst length and interval (so "68 GB/h, fills the drive in 3.4 h" is on screen when you choose);
- a collapsed *Stacking settings* with the two percentages and multi-point;
- a **live plan check** under the control: "Fits: a burst takes 1.8 s, stacking about 5 s, the next burst is 28 s away" or "Too tight: these bursts will be saved as video" (from the stacker's measured time and the ring's overwrite budget), updated as you change the interval or frame count;
- during a recording, the progress line shows per burst *recording, stacking, stacked*, with the stack's thumbnail as it appears, and the existing end-time estimate;
- **errors and fallbacks are visible, not silent:** see *Errors are never quiet* above: a banner with the reason until acknowledged, an entry in the session log, a count in the status bar.

**More > RAM buffer** (extended): the buffer as the ring it now is: size, how much is held by the writer, by the stacker, how many seconds of full-rate video it holds, and the *Wait for the disk / Drop frames* policy explained in one line each.

**More > Stacking** keeps the service settings and the list of stacks, and gains per-stack details: frames used, points, a quality score, whether video was kept, whether it was a fallback.

**Presets:** the existing Record presets gain a recommended mode (the eclipse preset uses mode 2).

Out of scope for now (decided): a schedule that keeps video only inside a time window, an audit sample of one burst in N, and a "keep raw now" button; the design leaves room for them (the burst table in the ring carries a mode per burst).

## The manuals

New:
- **Guide: Recording and stacking modes:** what each mode keeps, the disk per hour table, when to use which, the percentages, how the fallback works and what you will see, how to reprocess, a worked eclipse recipe and a worked everyday-solar recipe.
- **Guide: The RAM buffer and the frame ring:** sizing for 4 and 8 GB Pis, what the numbers on the page mean, Wait against Drop, why a recording needs the buffer even in stack-only mode.
- **Reference: the frame ring:** layout and protocol, for tools that want to attach as a consumer.

Updated with the stage that changes them: Record (the new section), Stacking (in-memory against the service), Setting up the Pi (buffer), Troubleshooting (a burst fell back to video; a stack lost its frames; the stacker is not running), the API reference (`stack_mode`, `stack_keep_percent`, `raw_keep_percent`, ring status, events), Architecture, the hardware checklist (real-camera tests of every mode), and the release notes. ADR 0019 becomes *Accepted* when the last stage is verified on the real camera.

## Order of work

| Stage | Engine | UI and manual in the same stage |
|---|---|---|
| 1 (done) | the ring | ADR, reference page |
| 2 (done) | the stacker reads frames from a ring or a SER, identical results | none (internal) |
| 3 (done) | the raw writer as a ring consumer, policies and counters preserved | RAM buffer panel and its guide page |
| 4 (done) | the stacker as a ring consumer over a socket, the overwrite budget, the fallback | the live plan check, the warnings, troubleshooting |
| 5 (done) | modes 1 to 4 and the percentages through the API | the "After each burst" section, the modes guide, API reference, Playwright tests, `scripts/sim/after-burst.sh`, the real-camera checklist |

Stage 3 changes the recorder's core buffer, so it gets the longest soak (simulator, then the stick, then the camera) before anything builds on it.
