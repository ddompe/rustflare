# What to test on the next sunny day

Everything below was built and tested on the simulators, and some of it was run once on the Sun on 2026-11-26. Each line says what to do, what to look at and when it counts as passed. The issue numbers are on the [board](https://github.com/ddompe/rustflare/issues). Version 0.8.2 is assumed.

## A clear night with the ASI533MC on the C11

Added 2026-11-28 while it was cloudy. Done indoors that day (see the [hardware checklist](hardware-checklist.md)): the USB 3 link, frame rate against region size, region rounding, recording 512 x 512 at 16-bit to the USB 2 drive. What needs sky:

1. **Real image scale and Barlow factor (the drift test).** Telescope in place, camera on the planned train. Aim at a bright star near the celestial equator, tracking **off**, record 20 s at a short exposure, note how many pixels N the star moves in T seconds: scale = 15.04 x T / N arcsec per pixel (times the cosine of the declination), focal length = 206.265 x 3.76 / scale mm (3.76 um pixels). Compare with the estimate (about 6000 mm, f/22, 0.13"/px for the C11 with the GSO 2x and the IR-cut adapter) and enter the result as the measured factor in the optical setup (*Barlow measured* / *Use measured*). Repeat for each train (native, Barlow threaded on the camera, Barlow element on the nosepiece).
2. **Planet focus assist (new).** A planet on the Planet profile, a region of about 512 x 512, 16-bit, an exposure that does not clip. Switch focus assist on and turn the focuser in small steps through focus and back, waiting 2 to 3 bursts at each position. Write down the score at each step. Passed when the history has one clear peak, the error band is smaller than the difference between in focus and a few turns out, and "you are at the best" appears at the peak. Also note: whether the planet is found (the note "No planet found..." must not appear), what happens with the planet drifting, and whether a clipped (white) planet scores visibly lower.
3. **Offset and black level.** Cap on, 16-bit, the exposure you will use: raise the offset until the sky's black peak in the histogram clears the left edge (a few hundred counts of 65535). Then take a dark: *Dark* in the overlay must say applied with nothing differing; change the offset by 20 and the Darks panel must say "offset".
4. **Calibration per camera.** Take a flat on the Barlow train (Record > Flats), then switch to the Apollo (or the other camera) and back: each camera keeps its own dark and flat and never gets the other's. Change the region position at the same size: the dark must report "region".
5. **A planet recording.** Go to the planet, Planet centroid tracking, a 512 x 512 region at 16-bit, bursts of 60 to 90 s (the drive writes about 28 MB/s, so bursts longer than about 2.5 minutes at 100 fps drop frames; or use 8-bit). Check the frame counts in the files against the camera's fps and `dropped` in the status.
6. **Seeing across bursts (for the per-frame quality idea).** Record 5 to 10 bursts of the same planet over some minutes and stack each one separately (AutoStakkert or Siril). Note which bursts look best: this is the reference to compare the future per-frame scores with.
7. **Region on the sky (#113).** With a planet in view, an odd region request (the planet near the corner): the colours stay right after the app rounds to an even corner, and the fps rises with the smaller region.
8. **The Moon with the 533** if it is up: the Moon profile, a region on a limb, 16-bit; the focus assist on the Moon (it still uses the Sun's surface-detail score; note how it behaves).

If the Sun is out as well: the tracker analysis time on the Pi (`GET /api/debug/state`, `perf`) for the limb-fit speed-up (#100), the cloudy case especially.

## Before the Sun

- **Drive on a black USB 2.0 port, camera on a blue USB 3 port.** More > Storage must show no USB warning. (Plug the drive into a blue port once to see the warning appear, then move it back.)
- **Power:** `vcgencmd get_throttled` says `0x0` and `journalctl -k | grep -i undervolt` is empty after ten minutes of live view. The supply used on 2026-11-26 negotiated only 900 mA (`max_current` in `/proc/device-tree/chosen/power`).
- **Logs on:** More > Session log (a picture every 10 s) and the sampler (`scripts/pi-sampler.py 6` on the Pi) so a bad moment can be reconstructed. The sampler now survives a service restart.
- **Free space:** at least 20 GB on the drive; delete test recordings from earlier days.

## In order of value

1. **Darkness guard (#96).** While guiding holds, cover the front of the telescope for 20 to 60 s (a cap, a cloth) or wait for a shadow or thick cloud. Look at: "lost" within a few seconds of the picture going dark, no pulses while it is dark (More > Session log: `pulses` stays empty, or `dead_reckoning` starts from the last bright position), and guiding resumes by itself when the light is back, with the Sun within a few tens of pixels of the target. Passed if the mount is not pushed (the Sun is where it was, plus the learned drift).
2. **Unattended automatic flip (#80).** Arm it 10 to 15 minutes before the meridian (Aim > Meridian flip) with a recording running. Look at: the position recorded 5 min before, the recording pausing, the flip, the Sun back near its place, the recording resuming. Then arm it again after the flip: it must say "no flip this time". Passed if the recording continues with the Sun in frame and nothing flips back.
3. **Guided full-rate recording on USB 2.0 and on the SD card (#86).** 3 minutes each at full frame with the drop policy. Look at `frame_gap` events in `logs/<date>/events.jsonl` (now with `waiting_for_camera_ms`, `prev_frame_work_ms` and the kernel's memory state) and `dmesg` for USB disconnects. Passed if there are no gaps over 0.5 s and no USB events. Compare the drive on USB 2.0 with More > Storage > Internal storage (the SD card keeps 8 GB free and stops itself before using it).
4. **Stacking from memory under pressure (#97).** Stack-only with bursts 6 s apart or less, a 300-frame burst, or the stacker slowed (`stack.json` threads 1). Look at the notices and the sidecars: a burst the stacker cannot finish before the camera wraps must be written as video with a notice, never lost.
5. **Prominence session (new raw lane).** `+ Prominence` with *Save the disc position with every prominence frame* on, a bright prominence at the limb, 3 to 5 cycles. Look at the manifest (`disc.how` is `fitted`), the FITS headers (`DISCX`, `DISCY`, `DISCR`) and the picture at disc size 100 % (More > Stacking). The radius the fit finds (1232 to 1238 px) is 0.3 to 0.8 % above what the optics give (#101): note what the focal length and pixel size entries are.
6. **Tracking rate after a Go to the Moon or a planet (#89).** Go to the Moon, then check the mount's `:GT#` (`GET /api/debug/mount-query?what=GT`): `1` for lunar, `0` for sidereal. Switch tracking off and on; the rate must stay. Return to the Sun (solar is `2`).
7. **Drift over an hour (#82).** Guiding off, tracking on, session log on, an hour with nothing touched. Look for the slide of about 330 px along Dec every 18 minutes seen on 2026-11-22. Passed if the Sun's path in `session_*.jsonl` is a smooth line.
8. **Focus assist with the focuser (#84).** Turn the focuser in small steps through focus and back, waiting 2 to 3 bursts at each position. Write down the score at each step: how far it moves per turn, and whether the grey band (±0.04 or so) is small against that.
9. **Camera drop (#95).** If the camera drops, note what the app showed: today it shows nothing and guiding stops. The fix is on the board.
10. **Stacker against other software (#91).** Take the comparison sets from the stick (`compare_disc`, 4 × 200 frames; `loadtestC2`, 10 × 100) and stack them with AutoStakkert or Siril. The on-device stacks are in `stacks/2026-11-26/`. Compare sharpness at the limb and the noise of a flat area.

## If time allows

- With the iOptron HAE69C: Go to Sun after the time and place were sent (Aim > Mount time and place), then Calibrate and Hold position (guide pulses `:ZS` `:ZQ` `:ZE` `:ZC` are not yet tried on it); see the [checklist](hardware-checklist.md).
- The Moon profile on the real Moon (Go to Moon, the outline against the disc, the flip target) and a planet in a small frame; the ASI533MC Pro's cooler and colour histogram if the camera is at hand (see *Still to check with light and sky* in the checklist).

- A Go to Sun from the other side of the meridian and the image rotation (#81).
- The drive pulled without ejecting during a recording: the app should say what happened in words and the next recording should refuse a read-only drive (#85).
- A recording to the SD card of 3 seconds at full rate and the files' frame counts.
- The wrap of a long sequence on a FAT32 drive (split before 4 GB) with the drive on USB 2.0.

## Known and not fixed

A camera drop stops the capture, the tracker and guiding without a message (#95); the guiding calibration and hold are lost when the service restarts (#99); `stacks/manifest.json` is the file a USB drop corrupts (#98); the tracker costs 9 to 30 ms per analysis on the Pi (#100).
