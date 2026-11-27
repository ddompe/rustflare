# What to test on the next sunny day

Everything below was built and tested on the simulators, and some of it was run once on the Sun on 2026-11-26. Each line says what to do, what to look at and when it counts as passed. The issue numbers are on the [board](https://github.com/ddompe/rustflare/issues). Version 0.8.0 is assumed.

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
5. **Prominence session (new raw lane).** `+ Prominence` with the coronagraph, a bright prominence at the limb, 3 to 5 cycles. Look at the manifest (`disc.how` is `fitted`), the FITS headers (`DISCX`, `DISCY`, `DISCR`) and the picture at disc size 100 % (More > Stacking). The radius the fit finds (1232 to 1238 px) is 0.3 to 0.8 % above what the optics give (#101): note what the focal length and pixel size entries are.
6. **Tracking rate after a Go to the Moon or a planet (#89).** Go to the Moon, then check the mount's `:GT#` (`GET /api/debug/mount-query?what=GT`): `1` for lunar, `0` for sidereal. Switch tracking off and on; the rate must stay. Return to the Sun (solar is `2`).
7. **Drift over an hour (#82).** Guiding off, tracking on, session log on, an hour with nothing touched. Look for the slide of about 330 px along Dec every 18 minutes seen on 2026-11-22. Passed if the Sun's path in `session_*.jsonl` is a smooth line.
8. **Focus assist with the focuser (#84).** Turn the focuser in small steps through focus and back, waiting 2 to 3 bursts at each position. Write down the score at each step: how far it moves per turn, and whether the grey band (±0.04 or so) is small against that.
9. **Camera drop (#95).** If the camera drops, note what the app showed: today it shows nothing and guiding stops. The fix is on the board.
10. **Stacker against other software (#91).** Take the comparison sets from the stick (`compare_disc`, 4 × 200 frames; `loadtestC2`, 10 × 100) and stack them with AutoStakkert or Siril. The on-device stacks are in `stacks/2026-11-26/`. Compare sharpness at the limb and the noise of a flat area.

## If time allows

- A Go to Sun from the other side of the meridian and the image rotation (#81).
- The drive pulled without ejecting during a recording: the app should say what happened in words and the next recording should refuse a read-only drive (#85).
- A recording to the SD card of 3 seconds at full rate and the files' frame counts.
- The wrap of a long sequence on a FAT32 drive (split before 4 GB) with the drive on USB 2.0.

## Known and not fixed

A camera drop stops the capture, the tracker and guiding without a message (#95); the guiding calibration and hold are lost when the service restarts (#99); `stacks/manifest.json` is the file a USB drop corrupts (#98); the tracker costs 9 to 30 ms per analysis on the Pi (#100).
