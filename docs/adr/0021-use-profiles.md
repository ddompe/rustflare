# 21. Use profiles: what is imaged, grey out with a reason, never hide

Date: 2026-11-26. Status: accepted (simulators only; nothing here has run on a real Moon or planet yet).

## Context

RustFlare began as a solar imaging server and assumed the Sun in its words, its defaults, its logic (detector, the one Sun model, the meridian flip, the guider's hold) and its metadata. The owner also images the Moon, and planets with a cooled colour camera in SER video mode with a small region of interest, stacked off the Pi. See `docs/project/proposal-use-profiles.md` for the inventory.

## Decision

1. A **profile** (`sun_white`, `sun_ha`, `moon`, `planet`, `other`) says what is imaged. The engine keeps it (`status.profile`, `PUT /api/profile`, saved in the settings file), because the engine needs it for the flip, the FITS `OBJECT`, the expected size, the file name and the detector. Existing installs default to `sun_ha`; `sun_*` is the application as it was, byte for byte.
2. A profile changes **words, defaults and which controls are enabled**. A control that does not apply is **greyed with the reason written under it, never hidden** (a phone has no hover, and a hidden control cannot be asked about). A test fails if one disappears.
3. **The API refuses nothing the UI greys out.** The profile adapts the screen, the headers and the engine's choice of measurer; it is not a lock.
4. **Nothing switches by itself.** Go to the Moon while on another profile offers the switch.
5. **One model of where the target is** (`sunmodel.rs`) stays the only one. The Moon and the planet feed it through their own measurer (`detect_target`: the disc and limb detector for the Sun and the Moon, `locate_planet` for a planet, nothing for `other`); the model gets an absolute pixel gate floor that is 0 for everything but the planet profile.
6. The meridian flip follows the **target** (`status.flip_target`): the Sun's numbers unchanged, the Moon at its own hour-angle rate (measured from its positions), a planet at the sidereal rate; after the turn the target's tracking rate is restored (solar, lunar, sidereal).
7. **No planet stacking on the Pi** (the stacker handles mono recordings; planets are stacked in AutoStakkert, PIPP or Siril from the raw SER). **No filter wheel for now.**
8. A planet is judged by its **peak** (`status.camera.peak`, the 99.99th percentile), not by the mean or the saturated fraction, which a planet at a tenth of a percent of the frame cannot move.

## Consequences

- The Sun detector does not find a planet: run on the simulated 25 px planet it returns a confident phantom disc of about 450 px radius. This is why the planet profile has its own measurer rather than a tuned threshold. On the Moon with a phase it takes the lit part for the disc (the centre moves toward the lit side, the radius shrinks): the Moon profile says so and the limb fit is right for a mostly full Moon only. Both are pinned by tests in `crates/server/src/target_tests.rs`; a robust lunar limb with a terminator is not done.
- Everything was built and tested on the simulators (a Moon with a phase, a planet with seeing jitter). Real seeing, a real planet's bands and moons, and the Moon's terminator on a real camera are unverified.
- The profile is state. A chip names it whenever it is not a Sun profile, and Go to offers the switch when target and profile disagree.
- Per-profile recording defaults, per-channel colour display controls and the dead band in arcseconds are not built.

## Addendum: an optical train carries a default profile (2026-11-26)

A named optical setup (focal length, Barlow or reducer factor, pixel size override, binning, aperture, notes, and the master flat and dark that belong to it) is saved in the settings file (`optical_setups`, the active name in `optical_setup`) and carries **the default profile of that train**: applying it sets the optics through the same code as `PUT /api/optics` and the profile through the same code as `PUT /api/profile`, so the engine learns the target class (expected radius, flip, measurer). A setup without a profile leaves the profile alone. The setup in use is `status.optics.setup` (`{active, modified}`); it is **modified** when the current optics, or the profile (when the setup has one), differ from it, and it is never overwritten without an explicit Save or Update. Nothing is created for an existing install. Applying moves no mount and starts nothing, and, like `PUT /api/optics`, is not refused while recording. A flat or dark that is gone is reported in the response and does not fail the apply. Not decided here: a dark's exposure and gain are not part of a setup, so a dark selected through a setup other than the one already loaded shows as not matching the camera until a dark is taken again.
