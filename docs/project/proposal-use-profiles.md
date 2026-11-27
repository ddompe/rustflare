# Proposal: use profiles (Sun, Moon, planets)

Status: proposal, 2026-11-26. Nothing in the code has been changed for it. Everything below was read from the tree at this date; where something was inferred and not run, it says so.

## 1. Summary

RustFlare assumes the Sun in four layers: **words** (about 60 strings in `web/src/`), **defaults** (exposure, the `limb` tracker, 100-frame recordings, orange H-alpha tint, "the Sun is taken as white"), **logic** (the detector, the one Sun model, the meridian flip and auto flip, focus assist, the guider's hold, the stacker's disc mask) and **metadata** (`OBJECT = Sun` in FITS, the `sky_sun` status block).

What already works for the Moon and planets: Go to (Sun, Moon, Mars, Jupiter, Saturn) with the right tracking rate, ROI, zoom, SER recording, the HDR lanes ("for eclipses and the Moon"), flats and darks, focus assist (with caveats), manual tracking rate. What is wrong, not just misleading: the **auto meridian flip and the flip assist follow the Sun's hour angle and restore solar tracking**, **FITS says OBJECT = Sun**, **auto white balance forces a coloured planet to grey**, **auto-stretch saturates a small planet**, **the stacker refuses colour (mosaic) recordings**, and **the detector very probably does not see a planet at all** (inferred from the thresholds, section 4).

Proposal in one paragraph: a **profile** (`sun_white`, `sun_ha`, `moon`, `planet`, `other`) lives in the engine status and in the settings file, is chosen at the top of the Camera tab (it is step 1, and the exposure defaults follow from it), and is always visible as a chip in the status strip. A profile changes **copy, defaults and which controls are enabled**. It never removes a control: a function that does not apply is greyed out **with a visible reason**, and the API stays permissive so tests and experts can still drive it. The existing installs default to `sun_white`, so every current Playwright test keeps passing unchanged. The smallest useful first step is UI-only (a `profile` UI setting plus a `Gate` component, copy and defaults; about a day), then the engine learns the target (flip, FITS, detector), then a planet tracker and display fixes, then colour (stacker, per-channel controls, filter wheel).

## 2. What the owner gets tonight with no code (ZWO camera, Moon)

Go to Moon (Aim tab, needs a saved site and a synchronised clock) slews and switches on **lunar** tracking; the rate buttons are there for anything else (`AimTab.tsx:359-388`, `engine.rs:1618-1642`). Then, by hand:

- Camera tab: pick the camera; **Colour** (if it is a colour model) set to **Manual** or **Off**; lower the exposure until the histogram stops at about 80 %. Ignore the red "No Sun found" chip.
- Track tab: leave tracking **off**, or use **Feature** on a crater. "Disc edge" and "Full disc" are built on a Sun-sized detector and a solar expected radius (section 4) and will do something plausible only near full Moon.
- Record: **Sequence**; keep **After each burst** on **Keep the video**. The stacker rejects a colour mosaic (`stack.rs:177-178`: "colour (mosaic) recordings are not supported yet"), so Stack only would fall back to video with a notice. Leave **Prominence view** off. Leave **automatic meridian flip** off (it would flip at the Sun's meridian time).
- Stack in AutoStakkert, PIPP or Siril; the SER is the raw RGGB mosaic with the pattern in the header, which is what they expect (`docs/reference/capture-formats.md:70`).

## 3. The colour question: what the live view does today

Sources: `crates/server/src/preview.rs`, `crates/capture/src/debayer.rs`, `web/src/components/ColourControl.tsx`, `docs/reference/capture-formats.md`.

| Topic | Today |
|---|---|
| Live view demosaic | **Superpixel**: every 2x2 block becomes one RGB pixel, the two greens averaged (`preview.rs:125-160`, `encode_bayer`). No interpolation, no fringes, so the preview has **half the sensor resolution**, then is divided again to fit `MAX_WIDTH`. Fine for framing and exposure; too coarse to judge planetary focus or seeing at full detail (zoom only magnifies the half-resolution picture). |
| Single-frame PNG | Saved twice: `_raw.png` (mosaic) and a **bilinear** debayered, white-balanced 16-bit colour PNG (`debayer.rs:19-46`, `engine.rs:3076-3083`). `capture-formats.md:76` still says "Not available yet: ... white balance controls, saving debayered PNGs": **out of date**, both exist. |
| White balance modes | `Auto`, `Manual`, `Off` (`ColourControl.tsx`, `preview.rs:23-35`). **Auto** (`debayer.rs:51-87`): average of the 2x2 blocks whose green is at least half of the brightest block, clamped 0.25 to 4, smoothed 0.8/0.2 per frame; the bright part is made neutral ("the Sun is taken as white", `ColourControl.tsx:16`). Fine for the Moon (nearly neutral). **Wrong for Mars, Jupiter, Saturn**: it measures the planet itself and removes its colour. |
| Per-channel gains | Red and blue only (green fixed at 1), range 0.25 to 4, step 0.05, `PUT /api/colour`. Applied to the live view and the colour PNG; never to recordings. |
| What the display tools do | `Brightness`, `Contrast`, `Gamma` are **global** (one value for all channels, CSS and SVG filter, `LiveView.tsx:57-70`). The `Colour` select is a tint (`Grayscale`, `H-alpha orange`, `Deep red`): on a colour camera `Grayscale` is just "no tint", not a greyscale. `Invert`. Auto-stretch uses one range for all channels (`stretch_range`, 0.5th to 99.8th percentile). |
| Recordings | **Raw mosaic.** SER with COLORID 8 to 11, FITS with `BAYERPAT`, `XBAYROFF`, `YBAYROFF`. Flats and darks are per photosite on the mosaic. Tracking and guiding run on the mosaic. Focus assist sums each 2x2 block (`focus.rs:46-53`). |

**Missing for planetary imaging**, in order of how much it matters:

1. **Per-channel levels or gamma and a saturation control** (live view and colour PNG only). Planetary colour is subtle and is always raised in processing; without it the preview of Jupiter or the Moon looks grey. Small: a 3x3 matrix or per-channel curve in `encode_bayer` plus sliders. Display-only, so it fits the "raw is the product" rule.
2. **Atmospheric dispersion**: nothing. No ADC support, no per-channel shift. At low altitude the red and blue discs separate by several pixels. Cheapest useful step: a live-view-only red and blue **shift** (dx, dy) so the user can see and judge the dispersion and tune an ADC; the real correction (RGB alignment) is done by AutoStakkert or Siril on the raw SER. A hardware ADC needs no software.
3. **A usable full-resolution view for focus**: a crop at 1:1 (the ROI exists, `RoiControl`) with a proper bilinear debayer in the preview, optional, because the cost is higher than the superpixel.
4. **Mono camera plus filter wheel and RGB or L-RGB capture**: **absent**. `grep` for a filter wheel finds nothing in `crates/`, `web/src/` or `docs/`. The HDR **lanes** (`CaptureLanes`, `engine.rs:1800-1830`, `BracketLanes.tsx`) are the right shape (several settings, cycled, one folder per lane), but a lane has an exposure and a gain, not a filter. It needs a `FilterWheel` trait in `rustflare-core` and a driver behind a Cargo feature (ZWO EFW SDK, as for the ASI camera), a `filter` field per lane, and a wait for the wheel to settle. Large; a separate ADR.
5. **Luminance**: no live luminance view (a greyscale view of a colour camera, which is what you focus on), no L-RGB workflow, no per-lane filter naming. The first is cheap (sum of the four photosites, as `focus.rs` already does); the rest depends on 4.
6. **Stacking colour**: the on-Pi stacker rejects mosaics (`stack.rs:177`, `source.rs:11`). For planets it is correct to leave that to AutoStakkert, but the UI must say so instead of failing per burst (section 6).

## 4. Inventory of Sun assumptions

Severity: **wrong** (gives a wrong result or a wrong action for the Moon or planets), **misleading** (right behaviour, wrong words or a wrong default), **harmless** (Sun wording, no effect). Moon column assumes a roughly full Moon; planets assume a disc of 20 to 60 px.

### 4.1 Engine, tracking, capture, stack (Rust)

| Where | Assumes | Effect on Moon / planets | Sev. |
|---|---|---|---|
| `engine.rs:983-1030` `step_auto_flip`, `status.sky_sun.minutes_to_meridian` (computed `engine.rs:1260-1267`, `flip.rs:meridian_timing`) | The meridian of the **Sun** and the Sun's day length (factor 1.002738) | Auto flip records and flips at the Sun's meridian time while the mount tracks the Moon or a planet. The AM3 stops tracking about 19 min after the **target's** meridian. Needs the target's hour angle (the Moon crosses about 50 min later each day, `bodies.rs` already gives RA and Dec). | wrong |
| `engine.rs:1083-1195` flip assist, `1105-1109` | After the flip, `set_tracking_rate(Solar)`, `mount_rate = Solar` | A flip during a Moon or planet session restarts the mount at the **solar** rate | wrong |
| `engine.rs:887-897` `snapshot_pre`, `1580` `carried_target` | Records the Sun's RA/Dec and carries the pointing by the Sun's motion | Wrong reference body | wrong |
| `engine.rs:3075`, `3155`; `capture/fits.rs:79` | `object: Some("Sun")` hard coded | FITS of a lunar or planetary recording say OBJECT = Sun | wrong |
| `tracker.rs:76-102` `view_gain` | Brightens a dim frame until the 99.5th percentile is 128 | A planet covers well under 0.5 % of the frame, so the 99.5th percentile is sky noise: noise is amplified, not the planet | wrong (planet) |
| `tracker.rs:104-205` `otsu_threshold`, `sky_edge_threshold` | The bright object fills at least 2 % of the frame (`above < 0.02*nn` returns `None`) and has a mean at least twice the threshold | A planet (0.1 to 0.5 % of a 1920x1080 frame) fails the 2 % test; Otsu on amplified noise is the only way left. **Inferred from the code, not run**: needs the sim planet scene to confirm | wrong (planet) |
| `tracker.rs:216-235` `find_disc` | At least 50 samples at a step of 4 (disc area at least 800 px, radius at least 16 px); equivalent radius from area | Small planets below about 16 px radius are never "found"; the centroid of a **gibbous or crescent Moon** is the centroid of the lit part, not the disc centre | wrong |
| `tracker.rs:254-266` `locate_sun`, `fit_limb_auto` (RANSAC on limb points) | The edge of the bright region is the limb | On a Moon with a terminator the terminator edge is also an "edge"; RANSAC with the expected radius probably picks the true limb arc, **untested** | misleading |
| `engine.rs:773-826` `expected_radius`, `status.rs:152-156`, `solar::angular_radius_arcsec` | Expected radius from the **solar** angular radius of the day | For the Moon (14.7 to 16.7 arcmin radius against the Sun's 15.7 to 16.3) it is within the 6 % prior only most of the month (`sunmodel.rs:RADIUS_PRIOR_TOL`); for planets (about 20 arcsec) it is plainly wrong and, because it is the prior of the limb fit, it fights the measurement | wrong |
| `sunmodel.rs:20-26` `GATE_RADII = 0.06`, `GATE_RADIUS`, `MAX_DRIFT` | Gates scale with the radius (a Sun of 300 to 1200 px) | A 25 px planet has a gate of 1.5 px; seeing alone moves it more, so measurements are "disagreeing" and the model re-locks constantly | wrong (planet) |
| `sunmodel.rs:82-87` `too_dark`; `engine.rs:2597`, `3607` `picture_is_dark` | "Good light" is learned when the mean level is above 0.05 | For a Moon at a low exposure, or any planet, the mean never reaches 0.05, so shadow and cloud protection never arms. Harmless on a dark night (no cloud shadow), but the sky is no longer protected from a passing cloud | harmless |
| `engine.rs:2457-2540` analysis loop (`locate_sun`, `learn_radius`, `feed_sun_model`) | A Sun is looked for in every frame at about 10 Hz | Burns about the same CPU for nothing when no Sun can be there; `status.sun` is the Moon centroid or `None` for planets, so the overlay, Aim offset and chips all follow it | misleading |
| `engine.rs:1728-1733` `CaptureProminence`, `1800-1830` lanes `prominence` kind, `3182-3244` `disc_track` | Prominence pair and prominence lanes need `status.sun`; the lane writes the Sun's position per frame | Meaningless off the Sun. `hdr` lanes are fine | harmless (a gated function) |
| `engine.rs:1500-1520` coronagraph; `preview.rs:11` | Black out the Sun's disc | Meaningless for planets; for the Moon it would hide the lit disc (the earthshine view could use it, but that is what the Totality lane preset does with exposure) | harmless |
| `engine.rs:2370-2400` focus assist; `focus.rs:1-60` | Score = RMS gradient of the disc **interior** (0.8 of the radius), limb excluded; window fixed in the frame, sized from the Sun's radius; notes "The Sun has moved out of the scoring window", "Is the Sun in view and the cap off?" | Moon: works (craters give texture) once `status.sun` is the Moon disc; words say Sun. Planet: interior is a few pixels, no limb-free texture, 0.8 of a 25 px radius is 20 px, `half = (r*0.6).clamp(24, 320)` makes a 48 px window; the metric is the wrong one (a planet wants edge or peak sharpness, or a star's FWHM) | misleading (Moon), wrong (planet) |
| `guiding.rs:89` `deadband_px = 3.0`; `engine.rs:2715-2730` dead reckoning | Pixel dead band tuned for a Sun-sized scale; "tracking lost for too long: guiding paused until the Sun is back" | At a planet's image scale 3 px is large; copy mentions the Sun | misleading |
| `engine.rs:540` `mount_rate: Solar` | Default rate is solar | Aim tab shows "Solar tracking" until a Go to changes it | misleading |
| `engine.rs:1965` flats | Flats "tracking meaningless" | Fine for any target | harmless |
| `stack.rs:177-178`; `stack.rs:1-20` rank by detail against the mean; `multipoint.rs:12` grid on structured parts, "disc" | Mono; one large textured disc; `keep` 25 %; `valid_mask` threshold 2.0 binned, erode 8 | Colour mosaics rejected. A small planet leaves almost no alignment points (a whole box must lie on signal). Defaults are tuned for 100-frame solar bursts | wrong (colour) / misleading |
| `stack/disc.rs`, `stack_api.rs:139`, `session.rs:88`, `output.rs:36-43` | `disc_mask`, `DISCX/Y/R` headers | Only for prominence lanes; absent otherwise | harmless |
| `capture/debayer.rs:51` `auto_white_balance` | The bright part is the Sun, which is white | See section 3: grey planets | wrong (planet), fine (Moon) |
| `preview.rs:94-123` `stretch_range` (0.5th / 99.8th percentile) | The object fills more than 0.2 % of the frame | A 0.25 % planet sits at the 99.75th percentile: the stretch's white point lands on the sky or the planet's edge and the planet clips to white | wrong (planet) |
| `files.rs:32` `mostly_dark` | Dark means "cap on, Sun out of view" | A planet recording is mostly dark **by nature**; the file manager shows "N mostly dark" (`FileManager.tsx:207`) for every one | misleading |
| `browse.rs:98`, `api.rs:295` folder types `prominence`, `hdr` | Names of sessions | fine | harmless |
| `core/frame.rs:60` "solar imaging is overwhelmingly done with mono cameras"; `FrameStats` has `mean`, `saturated`, histogram only | No peak or 99.99th percentile statistic | A planet needs "how bright is the brightest part", not the mean | misleading |
| `camera/simulator.rs:11,30,44-53,60-100` | One scene: a limb-darkened solar disc (`sim_scene` 0 = Sun, 1 = flat), "Simulated solar camera" | No Moon or planet to test with | misleading |
| `core/mount.rs:48`, `ioptron.rs`, `zwo.rs` | Rates `Sidereal`, `Solar`, `Lunar` exist | Good: no change | fine |
| `bodies.rs:11-20` | Targets: Moon, Mars, Jupiter, Saturn | Venus, Mercury (day or dusk, bright targets) and Uranus, Neptune cannot be selected | misleading |

### 4.2 Web UI copy, defaults and controls

| Where | Assumes | Effect | Sev. |
|---|---|---|---|
| `LiveView.tsx:49` | "Open the camera in the Camera tab to see the Sun." | Words | misleading |
| `LiveView.tsx:82-102`; `display.ts:16-17,26` `sunOutline`, `sunCentre` | Cyan circle and centre dot at `status.sun` | For the Moon it is the Moon (fine, words); for planets absent or random | misleading |
| `LiveView.tsx:114` Prominence view button (`disabled` when no `status.sun`); `MoreTab.tsx:43-44`; `Hud.tsx:25,255-264` mask widget | Software coronagraph | Not applicable; currently enabled whenever a bright disc is found, **including a Moon** | misleading |
| `LiveView.tsx:129`; `TrackTab.tsx:81` | "Tap a feature (sunspot, filament, plage)" | Words | misleading |
| `LiveView.tsx:67-69`; `MoreTab.tsx:50-51,205`; `display.ts:7` tint `H-alpha orange`, `Deep red` | H-alpha tint | Offered always; fine for a mono camera, harmless | harmless |
| `StatusStrip.tsx:17` clipping chip at `saturated > 0.1 %` ("reduce exposure or gain"); `Hud.tsx:210-217` | Clipping is any saturated area | Moon: right. Planet: 0.1 % of the frame can be a third of the disc | misleading |
| `StatusStrip.tsx:19` chip "No Sun found", "Too dark? Try more exposure" | Always shown with no detection | Permanent warning for planets; for the Moon, wrong advice when the Moon is not in the field | misleading |
| `StatusStrip.tsx:9` ("readable in sunlight"), `:12` "while the Sun is in view" | Comments | harmless | harmless |
| `StatusStrip.tsx:65` quick **Start** `recipeLabel`, `quickSize` (`recipe.ts:41`): "Start recording · 100 frames" | Default recipe 100 frames (`recipe.ts:18`, `capture.frames` default `'100'`) | 100 frames is 1 s of planetary video at 100 fps; planets and the Moon want thousands of frames or 60 to 180 s. The label is right, the default is a Sun default | misleading |
| `App.tsx:150-153` status bar: "Sun not visible: guiding goes on...", "Target lost: ... when the limb is back in view", "Sun hidden or unclear (cloud?)" | Sun and limb | Words | misleading |
| `CameraTab.tsx:163` "Pick your camera to see the Sun."; `:144` | Words | misleading |
| `CameraTab.tsx:197`, `ColourControl.tsx:16` | "the Sun is taken as white" | See section 3 | wrong (planet) |
| `CameraTab.tsx:201` Focus assist; `FocusAssist.tsx` | Interior texture | See 4.1 | misleading / wrong |
| `Flats.tsx:32` "no Sun disc"; `Darks.tsx:5,17` "same exposure and gain as the Sun frames", "Tracking stops while the dark is taken" | Words | Flats and darks apply to every target | misleading |
| `AimTab.tsx:36` "slew, track and hold the Sun in view"; `:129` big readout "No Sun found / Centred / N px off centre"; `:140` "Sun centre x, y, radius"; `:96-102` "Framing a region of interest" (partial disc) | Sun | Readout is useful for the Moon (a disc centre) but not for a planet unless a planet centroid exists | misleading |
| `AimTab.tsx:298-350` `GoToSun`: default target `'sun'` (`:300`), "Set your site first. Go to works out where the Sun is", "Sun now: altitude", `:341` text about tracking rate | Sun is the default and the wording | Words; the control already handles Moon and planets | misleading |
| `AimTab.tsx:374-376`, `MeridianFlip.tsx:8-41` "where the Sun was", "The Sun crosses the meridian in...", "Sun at x, y" | Sun's meridian | See 4.1 | wrong |
| `AimTab.tsx:378` `RATE_NAME[m.rate ?? 'solar']` | Default label "Solar tracking" | Words | misleading |
| `TrackTab.tsx:7-11,39` modes `limb` ("solar edge, typical for H-alpha"), `anchor` ("sunspot, filament or plage"), `sun_disc`, default `limb` | Solar modes, H-alpha default | Mode ids stay in the API; labels and the default change | misleading |
| `TrackTab.tsx:45,70-77,95-96,119,121,129` "tracking follows the Sun", "Disc radius", "Mount solar tracking", "Centre the disc instead", "as the Sun moves" | Solar tracking and the Sun's radius | The requirement (mount tracking on, calibrated) is right for every target; the label "solar" is not | misleading |
| `MoreTab.tsx:31` "the Sun position may be off"; `:76-89`, `:99-145` Optics ("the Sun's measured size implies an effective focal length", "Barlow/reducer matches the Sun's size") | The Sun as the ruler of the optics | Useful for the Moon (size known); for planets the apparent size is in the ephemeris but unused; the whole block says Sun | misleading |
| `RecordTab.tsx:120-139,175-178`, `ProminencePair.tsx` | "+ Prominence" kind, prominence exposure, "Save the disc position", "Stills: single frame, prominence pair" | Solar-only recording forms, always selectable | misleading |
| `recipe.ts:5`, `BracketLanes.tsx:5-8` presets "Partial phase", "Totality", ... | Eclipse presets | The Moon wants its own presets (see 5) | misleading |
| `RecordTab.tsx:123-124` "HDR lanes: for eclipses and the Moon" | | Correct | fine |
| `AfterBurst.tsx:42-85`, `Stacking.tsx:108-120` "Black disc on prominence stacks", "% of the Sun's radius", modes "Stack only" | Solar stacker defaults, mono only | Colour mosaics fail | wrong (colour) |
| `SessionLog.tsx:15-17` "last Sun position", "detected Sun" | Words and `sun` key in the log | Keep the log key for compatibility, change the words | harmless |
| `FileManager.tsx:20`, `:207` | Folder labels, "mostly dark" | See 4.1 | misleading |
| `api.ts:11,16,20,24,34` `SunLocation`, `sky_sun`, `sun_held`, `coronagraph`, `sun` | Names in the status | Leave the names, add `target` fields (section 8) | harmless |
| `datasheet.ts:35` demo mount rates | | | harmless |
| `web/e2e/ui.spec.ts` tests named "Go to Sun", "Prominence view", "prominence recording", "automatic meridian flip" | Sun default | Keep passing with the default profile | fine |

### 4.3 Documentation

`docs/guide/aim.md` (14 mentions; the Moon and planets get one subsection, `:17-23`), `track.md` (12; "The mount's own tracking follows the Sun's rate", the three modes, "If it loses the Sun"), `screen.md` (11), `troubleshooting.md` (10; "No Sun found", "Target lost"), `camera.md`, `record.md` (the "prominence" and "HDR lanes" text), `stacking.md` (prominence stacks, "disc"), `docs/reference/tracking-and-guiding.md` (18; the detector description), `file-formats.md` (9), `api.md` (11), `capture-formats.md:76` (outdated, see section 3), `README.md`. Also `AGENTS.md` ("One model of where the Sun is") needs one added sentence once the model is generalised.

## 5. The profile model

### 5.1 Profiles

| Id | Name | Target body | Camera | Detector / tracker | Notes |
|---|---|---|---|---|---|
| `sun_white` | Sun, white light | Sun | mono or colour | `locate_sun`, `limb` default, `sun_disc`, `anchor` | **Default**: today's behaviour, byte for byte |
| `sun_ha` | Sun, H-alpha | Sun | mono usually | same, `limb` default, anchor suggested when no limb, H-alpha tint offered, coronagraph and prominence lanes relevant | same engine path as `sun_white`; differs in defaults and copy only |
| `moon` | Moon | Moon (full or phase) | mono or colour | Moon disc/limb, expected radius from the Moon's ephemeris distance; `anchor` on a crater | lunar rate |
| `planet` | Planet | Mars, Jupiter, Saturn (later Venus, others) | usually colour or mono + filters | planet **centroid** (new), absolute-px model gate; no limb | sidereal rate |
| `other` | Other, stills | any | any | nothing automatic; no overlay, no tracking assumption | The neutral profile: nothing says Sun, nothing is disabled except what has no meaning without a body |

`sun_white` and `sun_ha` could be one profile with a sub-choice. Two ids keep the Segmented control honest and the defaults simple.

### 5.2 Where it lives

- **Engine owned, persisted in the settings file** (`config::set(storage.config_file(), "profile", ...)`, like `coronagraph` and `colour`, `engine.rs:1520`, `:1508`). Reason: the engine needs it (auto flip, FITS `OBJECT`, detector mode, expected radius, session log, sidecars), and it must be one truth for the phone and the laptop. A purely browser-side setting (`useSetting`, `settings.ts`, stored on the Pi under `/api/ui`) is fine for **stage 1** because the first stage is copy and gating only; stage 2 moves it.
- API: `GET`/`PUT /api/profile` `{ "id": "moon", "target": "moon" }`; `status.profile = { id, target, caps }`, where `caps` is the table in section 6 resolved to `{ feature: { ok, why } }`. Putting `caps` in the status keeps the reason texts in **one place** (Rust), tested once, and the UI renders them. Stage 1 duplicates it in `web/src/profile.ts`; stage 2 deletes that copy.
- Chosen on the **Camera tab**, first thing (a `Segmented`: "What are you imaging?"), because exposure and white balance defaults are step 1 and the tab order is Camera, Aim, Track, Record. Repeated in **More > Profile** for the details (target, expected size, what changed). A **chip** in the status strip always shows the profile ("Moon") and jumps to the Camera tab, like the other chips.
- It is **never changed by itself**. A Go to Moon while the profile is `sun_white` shows one non-blocking line in the status bar, "You are on the Sun profile: switch to Moon?" with a button. No silent switch: that is how defaults get surprising.
- Profile **defaults apply only to settings the user has not set**: `useSetting(key, def)` already returns the saved value or the default; the default becomes `profileDefault(profile, key)`. Settings are saved only when changed, so switching profile changes exactly the unchanged ones. A "Reset to profile defaults" button in More > Profile clears the profile's keys.

### 5.3 What a profile changes

| Property | `sun_white` | `sun_ha` | `moon` | `planet` | `other` |
|---|---|---|---|---|---|
| Go to default target, tracking rate on a fresh start | Sun, solar | Sun, solar | Moon, lunar | last planet, sidereal | none, unchanged |
| Tracker default mode | Disc edge (`limb`) | Disc edge | Disc edge if the whole limb or most of it is in view, else Feature | **Planet centroid** (new mode id `planet`) | Feature |
| Detector (`locate_sun`) runs | yes | yes | yes, with Moon radius prior | replaced by the planet centroid | no |
| Overlays default | Sun circle and centre dot, crosshair | same | disc circle and centre dot, crosshair | centre mark and crosshair, no circle | crosshair |
| Message copy | "Sun" | "Sun" | "Moon" | "planet" / the body's name | "target" |
| Exposure/gain hint (text, not forced) | "Aim the right edge of the histogram at 60 to 80 %" | same | "70 to 90 %; low gain; short exposure freezes the seeing" | "The planet is a small bright patch: the right edge of the histogram is the planet, aim at 80 to 90 %; raise gain, keep exposure short; crop with Frame size" | none |
| Clipping chip rule | `saturated > 0.1 %` | same | same | `peak` at full scale (needs a new statistic, section 7) | same as `sun_white` |
| White balance default | Auto | Off (mono) | Auto (the Moon is nearly neutral) | **Manual** (starts from the camera's own colours), Auto allowed but labelled | Off |
| Display default | no tint | H-alpha orange tint offered | no tint | gamma 1.0 to 2.2 left to the user, auto-stretch on a **peak** range | none |
| Recording default | 100 frames | 100 frames | 60 s, 1 recording | 90 s or 3000 frames, ROI crop suggested | 100 frames |
| HDR lane presets | Partial phase, Diamond ring, Totality | same | "Full Moon", "Crescent plus earthshine" (new) | none | none |
| After each burst default | Keep the video | Keep the video | Keep the video | Keep the video (stacking is off the Pi) | Keep the video |
| Stacker | usable | usable | usable for mono | **disabled for colour** (reason R-stack) | usable for mono |
| Flats and darks | white-light flat with the cap/diffuser | same | **relevant** (Moon is a large bright target; flats matter) | relevant (flats are the main fix for dust, with a small ROI the flat must match the ROI) | relevant |
| Auto meridian flip rule | Sun's hour angle, restores solar | same | the **Moon's** hour angle, restores lunar | the planet's hour angle, restores sidereal | off, disabled until a target is set |
| Focus assist metric | disc interior RMS gradient | same | interior texture (craters), same metric, wording changed | **new**: peak sharpness or edge steepness of the brightest blob, or the largest-contrast detail | frame-wide gradient |
| FITS `OBJECT` | Sun | Sun | Moon | the body | empty |
| Default file name | none | none | "Moon" if the name is empty | the body name if the name is empty | none |

Exposure numbers in the hint copy are standard practice, not measurements from this code base; the owner should confirm them for the camera.

## 6. Behaviour per element and profile

Legend: **on** = enabled; **off (Rn)** = visible, greyed, `aria-disabled`, with the reason text Rn **shown as a line under the control**, not only as a tooltip (a phone has no hover); **copy** = enabled with different words (the words are given). Reason texts follow the table.

| Element (file) | Sun (white / H-alpha) | Moon | Planet |
|---|---|---|---|
| **Profile chip and selector** (new, `StatusStrip`, `CameraTab`) | on | on | on |
| **Go to** selector and button (`AimTab.tsx:298`) | on, default Sun | on, default Moon | on, default last planet |
| Go to hint "Sun now: altitude" | on | copy: "Moon now: altitude, phase" | copy: "Jupiter now: altitude" |
| Tracking rate switch label (`AimTab.tsx:378`) | copy: "Solar tracking" | copy: "Lunar tracking" (follows the rate) | copy: "Star tracking" (sidereal) |
| **Meridian flip** disclosure, flip assist, **automatic flip** (`MeridianFlip.tsx`, `engine.rs:983`) | on, Sun | on, **Moon's** meridian, copy "The Moon crosses the meridian in..." | on, planet's meridian |
| Aim offset readout "N px off centre" (`AimTab.tsx:129`) | on, "No Sun found" | copy: "No Moon found" / "N px off centre" | off (R-offset) until a planet centroid exists, then on with "No planet found" |
| "Framing a region of interest" (`AimTab.tsx:96`) | on | on | off (R-partial) |
| **Track tab**, mode **Disc edge** | on, default | on, default for a mostly full disc, copy: "Fits the bright edge of the Moon; the terminator is not used" | off (R-limb) |
| Mode **Full disc** | on | on, copy: "Moon centre (full Moon: the whole disc in view)" | off (R-limb) |
| Mode **Feature** | on, copy "sunspot, filament or plage" | on, copy "a crater or a mare edge" | on, copy "a bright spot or a moon" (block matching; limited by seeing) |
| Mode **Planet centroid** (new) | off (R-planet) | off (R-planet) | on, default |
| "Disc radius" (`TrackTab.tsx:70`) | on | on, from the Moon's ephemeris | off (R-limb) |
| **Calibrate**, **Hold position**, guiding settings | on | on | on (dead band in arcsec, not px, as a follow-up) |
| "Mount solar tracking" step (`TrackTab.tsx:95`) | copy: "solar tracking" | copy: "lunar tracking" | copy: "tracking" |
| **Prominence view** button (`LiveView.tsx:114`), switch (`MoreTab.tsx:43`), mask HUD widget | on (H-alpha, white light with a coronagraph) | off (R-corona) | off (R-corona) |
| Sun edge circle and centre dot switches (`MoreTab.tsx:60-61`) | copy: "Sun edge circle" | copy: "Moon edge circle" | off (R-circle) |
| Tint select, `H-alpha orange` | on (H-alpha default offered) | on, left alone | on |
| Colour (white balance) segmented | Auto, Manual, Off | Auto (default), Manual, Off | Auto **copy**: "Makes the planet grey: only for checking", Manual (default), Off; text replaces "the Sun is taken as white" |
| Per-channel levels, saturation, RGB shift (new, section 3) | on if colour | on if colour | on if colour |
| Camera exposure presets / hint text | copy (Sun) | copy (Moon) | copy (planet) |
| **Histogram** and clipping chip (`Histogram.tsx`, `StatusStrip.tsx:17`) | on | on, hint "stop at 70 to 90 %" | on, hint on the right edge, rule by `peak` |
| "No Sun found" chip (`StatusStrip.tsx:19`) | on | copy: "No Moon found" | copy: "No planet found" only once a planet tracker exists, else not raised (R-nodetect appears in More > Profile) |
| **Focus assist** switch and graph (`FocusAssist.tsx`) | on, interior metric | on, copy "craters" | on, new metric; until it exists: on with copy "scores the brightest patch, not the disc: use with the zoom" |
| **Frame size (ROI)** (`RoiControl`) | on | on | on, hint "crop to the planet for frame rate" |
| **Flats** disclosure | on, copy "diffuser or flat cap" | on | on, hint "a flat is only valid for the same ROI and focus" |
| **Darks** | on, copy "same exposure and gain as the frames" | on | on |
| **Record kind**: Sequence | on, 100 frames | on, 60 s | on, 90 s |
| **Record kind**: + Prominence (and `ProminencePair`, `Stills: prominence pair`) | on (H-alpha, white light) | off (R-prom) | off (R-prom) |
| **Record kind**: HDR lanes | on, presets "Partial phase" and others | on, new Moon presets | off (R-lanes): a planet needs one exposure, not brackets; becomes on with the filter wheel (lanes per filter) |
| Repeating recording (bursts) | on | on | on |
| Quick **Start** label (`StatusStrip.tsx:65`) | "Start recording · 100 frames" | "Start recording · 60 s" | "Start recording · 90 s" |
| **Snap** | on | on | on |
| **After each burst** modes (`AfterBurst.tsx`) | all four | all four (mono), others (colour) see below | **Keep the video** only; Video and stack, Stack only, Best frames off (R-stack) when the camera is colour |
| Stacker settings, "Black disc on prominence stacks" (`Stacking.tsx:117`) | on | disc mask off (R-prom) | off (R-prom), stacker off for colour (R-stack) |
| Session log (`SessionLog.tsx`) | on | on, copy | on, copy |
| More > Optics block "the Sun is N in radius" (`MoreTab.tsx:99-145`) | on | copy: the Moon's radius today | copy: the planet's apparent size today |
| Files "mostly dark" flag (`FileManager.tsx:207`) | on | on | off (R-dark): flag suppressed for the planet profile |

### Reason texts

- **R-corona**: "Prominence view hides the Sun's disc. It does nothing for the Moon or a planet."
- **R-prom**: "Prominence recordings pair a disc exposure with a long exposure of the Sun's edge. Not used for the Moon or planets."
- **R-circle**: "No edge circle: a planet is too small to have a limb circle worth showing. The centre mark is on."
- **R-limb**: "A planet is only a few tens of pixels across: there is no edge to fit. Use Planet centroid."
- **R-planet**: "Planet centroid finds a small bright disc on a dark sky. Pick the Planet profile, or use Feature for a larger target."
- **R-partial**: "A planet never fills the frame, so there is no partial disc to frame."
- **R-offset**: "The offset needs a centre to measure. It turns on when Planet centroid finds the planet."
- **R-lanes**: "Exposure lanes bracket a bright and dark scene (an eclipse, the Moon). A planet needs one exposure. With a filter wheel, lanes will be one per filter."
- **R-stack**: "The Pi stacker only handles mono recordings. Record the raw colour video and stack it in AutoStakkert, PIPP or Siril." (This one also applies to the Sun with a colour camera: see below.)
- **R-dark**: "A planet recording is mostly black by nature; the dark flag is not shown for this profile."
- **R-nodetect** (copy line, not a control): "The Sun detector is off for planets: nothing marks the target until a planet centroid is found."

A rule that is not target-specific and should ship with this work: on **any** profile, if the camera is colour and the stacker is selected, say **R-stack** (it is a fact about the stacker, not about the target). The e2e test below checks it with the colour simulator.

## 7. What needs engine work and what is UI only

| Piece | UI only | Engine / server | Notes |
|---|---|---|---|
| Profile selector, chip, `Gate`, copy, defaults per profile | yes (stage 1, `useSetting('profile')`) | later: `Command::SetProfile`, `PUT /api/profile`, `status.profile` | `Gate` is a small component in `ui.tsx` next to `Switch` and `Segmented`: it sets `disabled`, `aria-disabled`, `aria-describedby` and renders the reason line; a click on a gated control shows the reason in the status bar (a native disabled button gets no click) |
| Gating of Prominence view, + Prominence, ProminencePair, mask HUD, Sun overlay switches, planet tracker modes, stacker modes for colour | yes | none | all are controls that already exist |
| Copy: chips, status bar, Aim, Track, Optics, Flats, Darks, Record, Camera | yes (words in `web/src/profile.ts`, one object per profile) | none | docs in section 9 |
| Recording defaults (frames, seconds), HDR presets, quick Start label | yes | none | `recipe.ts` takes the profile default when no value was saved |
| White balance default and wording | yes | none | the default is a setting that already exists (`colour` in the config); the profile only supplies it when unset |
| Auto flip and flip assist for the target's meridian, restore at the target's rate | no | **yes**: generalise `sky_sun` to a `sky_target` (the Sun or an entry of `sky_targets`), `meridian_timing` with the body's own sidereal factor, `snapshot_pre`, `carried_target`, `Solar` at `engine.rs:1105` becomes the target's rate | `status.sky_sun` stays for compatibility; the UI reads `status.flip_target` |
| FITS `OBJECT`, default file name | no | yes (`engine.rs:3075`, `3155`) | trivial once the engine knows the target |
| Expected radius for the Moon and planets | no | yes: ephemeris distance in `bodies.rs`, `Optics` fields `target_radius_arcsec`, `expected_radius_px` | the field `solar_radius_arcsec` is kept and filled for the Sun |
| Detector for the Moon (phase, terminator) | no | yes, tests first | maybe only the limb-fit prior and a "use the brightest arc" rule |
| **Planet centroid tracker** (`Tracker::planet`) | no | yes: local-background threshold, centroid of the brightest blob, minimum 3 px, feeds the same `SunModel` | the model must take an absolute-pixel gate (floor) so a 25 px planet is not gated at 1.5 px; the "one model of where the Sun is" rule stays: one model, now parametrised |
| Peak statistic in `FrameStats` (99.99th percentile), peak-based auto-stretch and clipping rule | no | yes (`core/frame.rs:74`, `preview.rs:94`) | auto-stretch for the planet profile uses the peak range |
| Focus metric for a planet | no | yes (`focus.rs`) | new function, `status.focus` gets a `metric` field |
| Stacker for a mosaic | no | yes, **large**: debayer then stack, or per-channel | default for planets is to leave it off the Pi; do not block the other stages on it |
| Per-channel levels, saturation, RGB shift in the live view | partly (sliders and a `ColourControl` section) | yes (`encode_bayer`, a matrix per channel, `PUT /api/colour` fields) | display only: recordings stay raw |
| Live luminance view | small | `encode_bayer` variant | |
| Filter wheel and L-RGB lanes | no | **large**: trait, driver behind a Cargo feature, lanes with a `filter`, ADR | not in the first stages |
| Simulator scenes | no | yes (`crates/camera/src/simulator.rs`, `sim_scene` 0 to 3) | see section 8 |
| Targets: Venus, Mercury, Uranus, Neptune | small UI | `bodies.rs` elements | optional |

## 8. Tests

### 8.1 Simulator scenes

`sim_scene` today is 0 (Sun) or 1 (flat); the camera is "Simulated solar camera" (`sim0`) and "Simulated colour camera (RGGB)" (`sim_osc`), both with the same scene control (`simulator.rs:30,60-100`). Add, without renaming the existing ids:

- **Scene 2, Moon**: a grey disc of radius `300 x zoom`, craters and maria as a few fixed blobs, a `sim_phase` control (0 new, 0.5 full, 1 new again) that shades the terminator with a soft edge. Displaced by `SimSky` like the Sun, so the mount, calibration and guiding scripts work on it.
- **Scene 3, planet**: a disc of radius 25 px (control `sim_zoom`), slightly oblate, two bands and a dark spot, one or two 3 px "moons" nearby, and a control `sim_seeing` (random jitter in px per frame, for the model gate and the focus metric). On `sim_osc`, a coloured disc (orange for "Mars", cream for "Jupiter") so the white balance test is meaningful.
- All still honour exposure and gain, so the histogram, clipping and stretch behave as real.

### 8.2 Rust tests

- `tracker.rs`: `locate_sun` on scene 2 at phases 0.5, 0.75 and 0.2 (centre and radius within a few px, or an honest `None`); on scene 3 `locate_sun` (document what it does today) and `Tracker::planet`.
- `sunmodel.rs`: a 25 px disc with 2 px jitter stays locked with the pixel gate; with the old gate it does not (regression).
- `flip.rs`: `meridian_timing` for the Moon and a planet (the factor), `carried_target` for a non-Sun reference.
- `engine.rs`: the auto flip uses the target's meridian; the flip restores the target's rate (simulated mount, a fake `now`).
- `preview.rs`: `stretch_range` on a planet frame (peak range) does not clip the planet; `auto_white_balance` on an orange disc is not neutral under the manual default.
- `capture/fits.rs`: `OBJECT` follows the profile.
- `status.rs`: `status.profile.caps` for every profile (the reason texts exist for every disabled feature; no feature is missing from any profile).

### 8.3 Playwright (`web/e2e/ui.spec.ts`)

Existing tests keep passing because the default profile is `sun_white`. Names to keep an eye on: "Go to Sun" (`:48`), "Prominence view toggles" (`:143`), "Record offers sequence, prominence and HDR lanes in one form" (`:165`), "the prominence recording with the coronagraph masks the disc in its lane only" (`:189`), "automatic meridian flip" (`:234`, `:620`), "focus assist" (`:206`), "mask size and focus score are overlay widgets" (`:271`).

New:

1. **Never hidden** (the rule, table driven): for each profile and each gated element in a list (Prominence view button, + Prominence, Stills prominence pair, Planet centroid, Disc edge, Full disc, Sun circle switches, Stack only, Best frames only), set the profile (API), then expect the element `toBeVisible()` and either `toBeEnabled()` or `toBeDisabled()` **and** the reason text visible in the page (`getByText(reason)`), on both the desktop and the `phone` project. A control that disappears fails this test.
2. **The profile persists**: choose Moon on the Camera tab, reload, chip says "Moon", `GET /api/profile` agrees.
3. **Copy**: Moon profile with the camera on scene 2: Aim shows "N px off centre" or "Centred", chip says nothing about the Sun, `getByText(/Sun/)` is absent from Aim, Track and the status bar (a regex allow-list for the genuine mentions).
4. **The Prominence view** is disabled on the Moon with R-corona, and enabled on the Sun (the same assertions as the existing test).
5. **Colour**: `sim_osc` with the planet profile: white balance is Manual, the Auto hint carries the new copy; with the Sun profile it is Auto and says "Sun is taken as white".
6. **Stacker and colour**: `sim_osc` and "Stack only": gated with R-stack, whatever the profile.
7. **Go to**: Go to Moon with the Sun profile shows the one-line "switch to Moon?" offer, and Go to does not change the profile by itself.
8. **Auto flip on the Moon**: the flip's "crosses the meridian in" text follows the Moon (compare with `status.sky_targets` RA).
9. **Record defaults**: Moon profile with nothing saved: the quick Start label says "60 s"; a saved value wins over the profile.
10. **API stays permissive**: with the Moon profile, `POST /api/coronagraph` still answers 200 (the grey-out is UI, the tests and experts can still reach it).

The scripts in `scripts/sim/` get one Moon and one planet scenario each (track, guide, record, check FITS or SER `OBJECT`/header), the same way `lost-target.sh` and `disc-mask.sh` are done.

## 9. Documentation

A new ADR ("Use profiles: disable with a reason, never hide"; the rule of the owner is the decision) plus: `docs/guide/camera.md` (the profile at the top), `aim.md` (the Moon and planets subsection becomes the profile description), `track.md` (mode names per profile, planet centroid), `record.md`, `stacking.md` (colour, R-stack), `screen.md` (chip), `troubleshooting.md` ("No Sun found" per profile), `reference/api.md` (`/api/profile`, `status.profile`), `reference/configuration.md` (the `profile` key), `reference/tracking-and-guiding.md`, `file-formats.md` (`OBJECT`), `capture-formats.md:76` (fix the stale sentence), `AGENTS.md` (one sentence in the Sun model rule).

## 10. Staged plan (smallest useful first)

**Stage 0: now, no code.** Section 2: Moon tonight with Go to, lunar rate, manual white balance, Keep the video. What fails today is listed in section 1.

**Stage 1: profile in the UI, copy and gating (about a day, UI only, no engine change).**
`web/src/profile.ts` (profiles, copy, defaults, reasons, `can(feature)`), the `profile` UI setting, selector on the Camera tab and More, chip in the status strip, `Gate` in `ui.tsx`, gate the controls of section 6 that need no engine support (Prominence view and its widget, prominence kind and pair, Sun overlay switches, stacker modes for colour, mosaic rule, tracker modes for the planet profile), change the copy, defaults for recording. Add the Playwright tests 1 to 7 and 9 to 10. Docs. **Visible result tonight:** nothing says Sun for the Moon, nothing is offered that does not apply, the reasons are on screen.

**Stage 2: the engine learns the target (about 2 days).**
`Command::SetProfile`, `status.profile`; auto flip and flip assist on the target's meridian and rate; FITS `OBJECT`; default file name; the expected radius from the body; "No Sun found" becomes "No Moon found" from the engine's own words; the engine stops running the Sun detector in `other`. Rust tests of 8.2 that concern flip and metadata; Playwright test 8. Removes the three "wrong" rows that can hurt a night (flip, solar rate restore, metadata).

**Stage 3: planets (about 3 days, riskiest).**
Sim scenes 2 and 3; tests on the current detector first (confirm the inference in section 4); `Tracker::planet` and the pixel-gate floor in `SunModel`; a peak statistic, peak-based auto-stretch and clipping rule; `mostly_dark` suppression; focus metric; dead band in arcsec. Lunar phase robustness for the detector.

**Stage 4: colour for planetary work (about 3 days for the display part).**
Per-channel levels or gamma, saturation, RGB shift, luminance view in `encode_bayer`; Moon HDR presets; optional planetary stacker via debayer.

**Stage 5: mono and filter wheel (large, separate ADR).**
`FilterWheel` trait, ZWO EFW driver behind a feature, lanes per filter, L-RGB recipes, naming of files by filter.

## 11. Risks and open questions for the owner

Risks:

- **Greyed controls are a promise.** Every gated control must have a test that proves it exists and says why (test 1). Without it the next refactor hides one.
- **The profile is state.** If the Moon profile stays on the next morning, the Sun UI is greyed. Mitigation: the profile chip is always visible, the Go to offer ("switch to Sun?") appears whenever the target and profile disagree, and nothing is applied silently.
- **A second place for reasons.** Stage 1 keeps the reasons in the web code, stage 2 in the engine status. The move must delete the first copy.
- **"One model of the Sun" (AGENTS.md).** The planet tracker and the Moon radius prior must feed the same `SunModel`, parametrised, never a second estimator in the UI or the mask. Rename later (`TargetModel`) only if the diff is small.
- **The detector claims for planets are inferred**, not run: stage 3 starts with a test that shows what really happens.
- **Defaults hurt when they are wrong.** The exposure numbers and the 60 s / 90 s recordings need the owner's judgement; the colour defaults (Manual for planets) are a position, not a measurement.
- **Sun and H-alpha split** adds a profile for little engine difference. Easy to merge later.

Questions:

1. Is the default for existing installs `sun_white`, with no prompt? (Proposed: yes.)
2. Do you want the Go to to **offer** the profile switch, or just a line in the status bar?
3. Should the API refuse what the UI greys out? (Proposed: no, so the tests and the sim scripts stay free.)
4. Planetary camera: which model and what frame rate? It decides the ROI hint, the default recording length, and whether the superpixel preview is good enough or a full-resolution debayer is wanted for focus.
5. Is on-Pi stacking of planets wanted at all, or is the answer "record raw, stack on the PC" (then R-stack stays and stage 4 skips the stacker)?
6. Mono plus a filter wheel soon? It is the biggest item and the only one that changes the recording model.
7. Which more bodies for Go to: Venus, Mercury, Uranus, Neptune? Cheap in `bodies.rs`, a profile target list in the UI.
8. The `status.sun` field name: keep for compatibility (proposed), add `status.target` as an alias, never rename in the log format (session logs and `analyze_ser.rs` depend on it).
