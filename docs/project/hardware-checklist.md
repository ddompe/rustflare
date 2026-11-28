# Hardware verification checklist

Everything below was built and tested on the simulators; each item needs the real device. The version on the Pi is shown at the right of the status bar (or `dpkg -s rustflare`); the software at the time of writing is `0.8.2`. Tick off an item, then close its GitHub issue.

## Verified on real hardware

- [x] ZWO AM3 over USB and Bluetooth: connect, tracking, nudge, **Send home**, **Go to Sun** (owner, 2026-11-21)
- [x] 4 GB FAT32 split, byte exact (#4)
- [x] Camera and USB stick on separate USB 3 controllers: ~57 fps held while a 3.4 GB sequence flushed (#58)
- [x] Pi clock NTP-synchronised, hardware RTC present
- [x] 2026-11-22 (owner): BLE connection to the AM3, Go to Sun, darks applied, flat applied in the overlay
- [x] 2026-11-22: safe eject of the FAT32 stick, tracking on real H-alpha with a Lunt LS50, the pointing readout (`:GAST#`) within 0.3 degrees of the Sun, the mount command log, guiding through a cloud pass (dead reckoning), focus assist (feedback in #78), the 60-second pointing check, and a 10-minute hotspot cycle joined from the phone
- [x] 2026-11-26/27: iOptron HAE69C over USB: `:MountInfo#` answers `0066`, state, tracking on and off, stop (replies take 300 to 600 ms: reply timeout 2 s), slews, Send home, Search home and the site and clock read and written (its clock had never been set: 2017-01-01; it has no battery). Guide pulses and Go to Sun are still to check.
- [x] 2026-11-28: **ZWO ASI533MC Pro on the Pi 5** (cooler off): needs a **USB 3** link. On a USB 2 port, or with a cable that only negotiates USB 2 (`lsusb -t` shows 480M behind the camera's own hub), it gave 4.5 fps at full frame; on USB 3 (5000M on bus 2 or 4) it gives 19.9 fps. Frame rates, 8-bit and 16-bit alike (the sensor readout is the limit, not the link): 3008 x 3008 19.9 fps, 2048 29, 1024 57, 512 110 (at 1 ms exposure; 97 fps at 10 ms, which is 1 / exposure), 256 205, 128 361, 64 583. The region rounds as designed (width to a multiple of 8, corner to an even value; under 64 or off the sensor is refused; the Bayer pattern stays RGGB). Camera controls survive a reconnect (#113 partly: the colours after an odd corner request and the picture itself are still to look at).
- [x] 2026-11-28: **recording 512 x 512, 16-bit, 110 fps to the USB 2 drive**: 9883 frames in 90 s, none dropped, all in the file (4942 MB). The drive writes about **28 MB/s** (the queue grew by about 52 frames a second and drained at about 54 frames/s afterwards), so a sustained run of this size is safe for about 2.5 minutes with the 8192-frame buffer; longer needs 8-bit (half the data), a smaller region or bursts with pauses.
- [x] ZWO ASI174MM Mini on a Pi 5, libasi 1.27: open, controls and frames, on a USB 2 port (8.7 fps at 16-bit; the USB 3 rate is still to measure, #30)
- [x] 2026-11-22: real prominence-lane recordings with the coronagraph (they exposed the Sun-detection and mask problems fixed the same day; a clean run after the fixes is still to be checked)

## Found on the Sun on 2026-11-22 and fixed (to re-verify)

Light dropping a hundredfold in seconds from palm-tree shadows made the old detector pick shadow edges as the limb, the mask jump, the tracker flap and guiding wander (median error 37 px, spikes to 1470 px). Fixed with brightness-invariant detection, one Sun model shared by outline, tracking, mask and guider, learned drift with dead reckoning, and median-based calibration. See #79. Debug tools: [Debugging and logs](../reference/debugging.md).

Also found on 2026-11-22, all fixed:

- the AM3 ends a guide command after 3 s, so calibration under-read the guide rate by a quarter and long lane-mode pulses were mostly ignored (#59);
- `:GAST#` answers `date#RA&Dec#flags#` and the driver stopped at the first `#`, so no flip feature could ever record the pointing (#80);
- the first meridian flip attempt was disabled by the owner; the mount stopped tracking by itself 19 minutes past the meridian, and a Go to Sun afterwards turned it over (image rotated by about 209 degrees).

Open: the Sun slid about 330 px along the secondary (Dec) axis three times, roughly every 18 minutes, before the polar alignment was redone (drift fell from 1.5 to 0.14 px/s after). If it comes back, read `mount.jsonl` for what preceded it; see #82.

## M1: field use

| Issue | Check | How |
|---|---|---|
| #66, #70 | Safe eject | More > Storage > *Eject drive safely* while idle ("Safe to remove"); try it while recording (refused); replug the stick, storage re-detects it |
| #57 | USB mount recovery | With the mount on USB: `echo 1-2 \| sudo tee /sys/bus/usb/drivers/usb/unbind` then `bind`; the banner clears and tracking state is read back within ~10 s |
| #68 | Flats | Diffuser on, Record > Flats > *Start flats...*: exposure converges to ~50 %, master flat written, the live view evens out, overlay shows *Flat: applied*; restart the service, it is remembered |
| #59 | Guider on a real sky | Calibrate, *Hold position* for 20+ minutes; no oscillation, few pulses per minute; try the Track > *Guiding settings* |
| #3 | Tracking on real H-alpha | limb tracker finds the partial disc; radius matches Optics |

| new | Darks | Cap on, Record > Darks > *Capture darks (cap on)*: hot pixels disappear from the live view, overlay *Dark: applied*; change the exposure and it says *mismatch* |
| new | FITS and drop policy | Record 100 frames as FITS (Record > Advanced > File format; open one in a viewer: image upright, EXPTIME and GAIN present); on the USB stick at full rate try *Keep camera speed* and check the dropped count |
| new | Buffer reuse | With the phone tab closed and then open, `top -H -p $(pgrep -x rustflare-serve)`: compare CPU with the ~55 % measured before ([performance audit](performance-audit.md)) |
| #72 | HTTPS on the iPhone | The Pi already serves HTTPS: install and trust the CA ([phone setup](../guide/phone-setup.md)), open https://<pi-name>.local:8443, Home Screen, location button |

| new | Session log | More > Session log on during a tracking/guiding session (20+ min on the Sun): the log appears under `<drive>/logs/`; copy it off and run the replay test ([development guide](development.md#real-data-regression-fixtures-session-log)); it should pass |

| new | ROI | Camera > Frame size: 50 % and 25 % in the phone UI; fps overlay jumps; tracking restarts; back to Full (measured via the API on 2026-11-21: 57 -> 153 fps) |

| new | Hotspot | At home, in a terminal with an ethernet cable or the Pi's screen if possible: `sudo rustflare-hotspot create`, then `up --yes --minutes 10`; join the Wi-Fi with the iPhone and open http://10.42.0.1:8080; it returns to the home Wi-Fi by itself (tested on the Pi with a 2-minute cycle on 2026-11-21; no phone was available to join it) |

| new | Hotspot with a USB Wi-Fi dongle | Plug a dongle; `sudo rustflare-hotspot status` shows 2 radios; `create` then `up`: the hotspot comes up on the dongle while ssh over the home Wi-Fi keeps working |
| new | iPhone and HTTPS landing page | Open http://<pi-name>.local:8080 on the phone: the page says whether HTTPS works and lists the three steps; make sure *Certificate Trust Settings* is switched on for RustFlare local CA |

| new | Meridian flip assist | Near the meridian with the Sun tracked and calibrated: Record position, Flip and re-centre (watch the mount); check the image turns 180 degrees, the Sun returns to its recorded spot, and `:GAST#` pointing is read correctly. Also try the manual path: flip with the ZWO app then Restore |
| new | Pause and resume | Record lanes for a minute, press Pause (Record tab or the bar under the picture), then Resume recording: new `-resumed` files continue with the cycles that were left; Discard forgets it |
| new | Mount command log | After a session `GET /api/debug/mount` (or `logs/<date>/mount.jsonl`) lists every command with its reply; check the 59 ms Bluetooth round trips and that nothing is sent that you did not ask for |
| new | Pointing readout (`:GAST#`) | Idle camera, Sun in view: Aim > Meridian flip > Record position works and shows RA and Dec close to the Sun's (15.88 h, -20 degrees in late November) |
| new | Automatic meridian flip (unattended, guiding on) | Turn on *Flip automatically* (Aim, Meridian flip) a few minutes before the Sun crosses the meridian. Expect: position recorded, flip just after the crossing (or when the mount stops tracking), Sun re-centred, recalibration if guiding, solar tracking back on |

| new | HDR bracketing lanes | Record 3 lanes for a minute on the Sun: check one folder per lane, the exposure in each file's notes, that the first kept frame of each burst really has the new exposure (no leftover brightness), and that the exposure is restored |

| new | Colour camera (OSC) | With a colour Player One or ASI: the header says colour with the pattern; the live view is in colour with plausible colours (pattern right: the Sun is not blue); a SER opens in AutoStakkert/Siril as the right Bayer pattern; ASI colour path has never run on hardware |

| new | Focus assist | In bright sun with tracking on: switch Focus assist on, turn the focuser through focus and back: the score should rise to a clear peak and the best line should match what you judge as sharp; check that it is not fooled by seeing or by drift of the Sun, and whether 1 s bursts are the right length |

## ZWO camera night (mono ASI174MM Mini and a colour ASI)

| Issue | Check |
|---|---|
| #30 | Mono ASI: open, exposure and gain, fps at full frame; USB 3 cable and blue port; ROI 50 % and 25 % (#76) |
| #56 | Colour ASI: header says colour with the pattern; the live view colours are plausible; a SER opens in Siril/AutoStakkert with the right Bayer pattern |
| new | Aim > Go to: pick Moon, Jupiter, Saturn, Mars in turn (the selector shows altitude; one below the horizon is greyed with its reason: the screen never sends the mount below the horizon). Check the mount arrives within a degree or two, tracking starts (lunar rate for the Moon), and how far the target is from the frame centre (note it: the planets use approximate elements) |
| new | Record a short planetary video (SER, short exposure, high gain) and check it plays; try *Home when finished* at the end of a counted recording |

## Storage by UUID

| Check | How |
|---|---|
| A renamed mount is followed | Eject the stick (More > Storage), replug it; if the desktop mounts it under another name (`...1`), More > Storage still shows it available with the same folder and a recording starts (checked once on 2026-11-22: `ASTRODRIVE` became `ASTRODRIVE1` and the saved choice followed it after the upgrade) |

## Frame ring and raw writer (ADR 0019, stage 3)

| Check | How |
|---|---|
| Files are intact | Record bursts and a long sequence with the real camera: every SER has the requested frame count, size = 178 + frames x (frame + 8), increasing timestamps, split at 750 frames on FAT32 (done 2026-11-22, Pi 5, Apollo-M Mini) |
| Wait mode | A recording longer than the buffer with *Wait for the disk*: nothing lost, the camera slows to disk speed (done: 57 fps down to 11 fps, 1200 of 1200 frames) |
| Drop mode | The same with *Drop frames*: a note on the Record tab, an entry in the event log, the burst's `.json` says how many were lost, and written + dropped = captured (done: 1053 + 147 = 1200) |

## After each burst (ADR 0019, stage 4)

| Check | How |
|---|---|
| Each mode with the real camera | Record 5 bursts at the Sun's exposure in each of the four modes (Record > After each burst): stack-only leaves stacks and no video, best frames leaves 25 % of the frames, video + stack leaves both (done 2026-11-22 on a dark scene, Pi 5 and Apollo-M Mini, 60-frame bursts: 15 of 60 frames kept, stacks in 4.6 s each; on the Sun still to do) |
| Stack time against the real Sun | The plan line uses the measured seconds per frame; compare with a stack of the same SER from disk (a real disc is slower than the noise a camera sees indoors) |
| No frame loss with a stack running | `frame_gap` events and `dropped` stay at zero for an hour of 30 s bursts in stack-only mode (done for 16 bursts of 100 frames, 8 minutes, plus 8 bursts 6 s apart: zero gaps, zero drops, RAM peak 2 GB with a 4 GB buffer; an hour on the Sun still to do) |
| Fallbacks are visible | Stop the stacker mid-recording (`sudo systemctl stop rustflare-stack`): the status bar warns and stays until dismissed, and every burst has a video or a stack |

## Recorder priority and stalls

| Check | How |
|---|---|
| Stacker does not disturb the real camera | Record on the Sun with the Apollo at full rate for an hour with the stacker on `between_bursts`, then with `always`; compare `frame_gap` events in `events.jsonl` and the recorder's dropped frames with the job table of `stack_telemetry.py` (the 2026-11-22 run used a simulated camera only) |
| Memory limit enforced | After `sudo rustflare-memcg` and a reboot: `cat /sys/fs/cgroup/cgroup.controllers` lists `memory`, `/sys/fs/cgroup/system.slice/rustflare-stack.service/memory.max` is 2147483648 (done on the development Pi 2026-11-22) |
| Priorities took effect | `ps -L -o tid,comm,ni,psr -p $(pgrep -x rustflare-serve)`: nice -5 on the recorder, nice 19 on `logwriter`; the stacker shows `Cpus_allowed_list: 2-3` in `/proc/<pid>/status` |

## Stacker (rustflare-stack)

| Check | How |
|---|---|
| Stack alongside a real recording | `sudo systemctl enable --now rustflare-stack`, then run lane recordings with bursts every 30 s for an hour on the Sun: `stack-status.json` shows *paused* while a burst writes and *working* in the gaps; no dropped frames and no capture stalls longer than before (#86); temperature stays under 70 C |
| Quality on a real flat and dark | Take a raw flat and a dark at the recording's exposure and gain (Camera tab), set them in `stack.json`, compare a stack with and without (disc texture, dust, hot pixels) |
| Compare with AutoStakkert or Siril | Stack one disc burst and one prominence burst in your tool and with `rustflare-stack session`; compare the FITS by eye and tell us which is sharper |

## M2: UI

| Issue | Check |
|---|---|
| #50 | Home Screen app on the iPhone: safe areas, sunlight legibility, pad usable with a thumb, overlays draggable, opacity slider |
| #69 | Record with the cap on (dark) and on the Sun; Files > *Check N recordings* flags the dark one; delete both |
| #63 | Second full UI review on the phone after the overlay changes |

## M3 (last)

| Issue | Check |
|---|---|
| #30 | ASI174MM Mini on a USB 3 cable and blue port: open, set exposure/gain, fps at full frame and ROI |

## iOptron mount driver (0.7; a real HAE69C connected on 2026-11-26 and 27, the rest mock only)

| Check | How |
|---|---|
| Connect | Aim > Mount `ioptron`, the mount's USB serial port: the card names the model (`:MountInfo#`). CEM70 expected `0070`; a real HAE69C answered `0066` (done, 2026-11-26) |
| State | `GET /api/debug/mount-query?what=GLS` while tracking, slewing and parked: the status digit matches what the mount does |
| Site and time | Go to Sun sends them; the mount's hand controller shows the right place and UTC time |
| Mount time and place (#104) | Aim > *Mount time and place* shows the mount's site and clock (the HAE69C's clock was seen unset, 2017-01-01) and the chip says it differs. **With someone at the mount**, press *Send site and time to the mount* (set commands, no motion): the block reads "matches", the hand controller shows the Pi's place and UTC time, and the clock keeps ticking (look again after a minute). On an AM3: the clock is read from `:GAST#` (the mount's local time), the place shows "not reported" |
| Go to Sun | Slews to the Sun, then tracks by itself; `:RT2#` (solar) is selected first |
| Pad | N, S, E, W move the image the way the buttons say (if not, the RA and Dec signs in `ioptron.rs` `slew` are swapped) |
| Guide | Calibrate and Hold position: pulses of 2.5 s, rate `:RGrrdd#`, no runaway |
| Send home | `:MH#` goes to the zero position; `state` then reports at home |
| Meridian | Past the meridian: a new Go to flips (the mount's `:GMT#` setting decides) |

## Still to check with light and sky (0.8.0)

| Check | How |
|---|---|
| Colour histogram with real light | ASI533MC Pro pointed at the Moon or a bright scene: three curves, the clipping marker per channel appears before the picture clips |
| Moon profile | Go to Moon: lunar rate, the Moon outline radius against the real disc, automatic flip names the Moon |
| Planet profile | a planet in a small ROI: the planet locator marks it, peak-based stretch, Planet centroid tracking holds it with the slew pad nudging |
| Cooler | switch on at -10 C, power and temperature move, switch off ramps about 4 C a minute, snowflake shows while on (not yet tried: the cooler has only been off) |
| iOptron guiding | Calibrate and Hold position on the HAE69C (guide pulses `:ZS` `:ZQ` `:ZE` `:ZC` are not yet tried), Go to Sun after the time sync |
| Go to on the HAE69C | Go to Sun, then a meridian pass: the mount's own meridian setting decides |
| Optical setups | save the real trains, apply one with its profile, flat and dark |

