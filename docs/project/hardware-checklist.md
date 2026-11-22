# Hardware verification checklist

Everything below was built and tested on the simulators; each item needs the real device. The installed build on the Pi is `0.2.6+dev…` (see `dpkg -s rustflare`). Tick off an item, then close its GitHub issue.

## Verified on real hardware

- [x] ZWO AM3 over USB and Bluetooth: connect, tracking, nudge, **Send home**, **Go to Sun** (owner, 2026-11-21)
- [x] 4 GB FAT32 split, byte exact (#4)
- [x] Camera and USB stick on separate USB 3 controllers: ~57 fps held while a 3.4 GB sequence flushed (#58)
- [x] Pi clock NTP-synchronised, hardware RTC present

## M1: field use

| Issue | Check | How |
|---|---|---|
| #66, #70 | Safe eject | More > Storage > *Eject drive safely* while idle ("Safe to remove"); try it while recording (refused); replug the stick, storage re-detects it |
| #57 | USB mount recovery | With the mount on USB: `echo 1-2 \| sudo tee /sys/bus/usb/drivers/usb/unbind` then `bind`; the banner clears and tracking state is read back within ~10 s |
| #68 | Flats | Diffuser on, Camera > Flats > *Start flats*: exposure converges to ~50 %, master flat written, the live view evens out, overlay shows *Flat: applied*; restart the service, it is remembered |
| #59 | Guider on a real sky | Calibrate, *Hold position* for 20+ minutes; no oscillation, few pulses per minute; try the Track > *Guiding settings* |
| #3 | Tracking on real H-alpha | limb tracker finds the partial disc; radius matches Optics |

| new | Darks | Cap on, Camera > Darks > *Capture darks*: hot pixels disappear from the live view, overlay *Dark: applied*; change the exposure and it says *mismatch* |
| new | FITS and drop policy | Record 100 frames as FITS (open one in a viewer: image upright, EXPTIME and GAIN present); on the USB stick at full rate try *Drop frames* and check the dropped count |
| new | Buffer reuse | With the phone tab closed and then open, `top -H -p $(pgrep -x rustflare-serve)`: compare CPU with the ~55 % measured before ([performance audit](performance-audit.md)) |
| #72 | HTTPS on the iPhone | The Pi already serves HTTPS: install and trust the CA ([phone setup](../guide/phone-setup.md)), open https://<pi-name>.local:8443, Home Screen, location button |

| new | Session log | More > Session log on during a tracking/guiding session (20+ min on the Sun): the log appears under `<drive>/logs/`; copy it off and run the replay test ([development guide](development.md#real-data-regression-fixtures-session-log)); it should pass |

| new | ROI | Camera > Frame size: 50 % and 25 % in the phone UI; fps overlay jumps; tracking restarts; back to Full (measured via the API on 2026-11-21: 57 -> 153 fps) |

| new | Hotspot | At home, in a terminal with an ethernet cable or the Pi's screen if possible: `sudo rustflare-hotspot create`, then `up --yes --minutes 10`; join the Wi-Fi with the iPhone and open http://10.42.0.1:8080; it returns to the home Wi-Fi by itself (tested on the Pi with a 2-minute cycle on 2026-11-21; no phone was available to join it) |

| new | Hotspot with a USB Wi-Fi dongle | Plug a dongle; `sudo rustflare-hotspot status` shows 2 radios; `create` then `up`: the hotspot comes up on the dongle while ssh over the home Wi-Fi keeps working |
| new | iPhone and HTTPS landing page | Open http://<pi-name>.local:8080 on the phone: the page says whether HTTPS works and lists the three steps; make sure *Certificate Trust Settings* is switched on for RustFlare local CA |

| new | Meridian flip assist | Near the meridian with the Sun tracked and calibrated: Record position, Flip and re-centre (watch the mount); check the image turns 180 degrees, the Sun returns to its recorded spot, and `:GAST#` pointing is read correctly. Also try the manual path: flip with the ZWO app then Restore |
| new | Automatic meridian flip (unattended, guiding on) | Turn on *Flip automatically* (Aim, Meridian flip) a few minutes before the Sun crosses the meridian. Expect: position recorded, flip just after the crossing (or when the mount stops tracking), Sun re-centred, recalibration if guiding, solar tracking back on |

| new | HDR bracketing lanes | Record 3 lanes for a minute on the Sun: check one folder per lane, the exposure in each file's notes, that the first kept frame of each burst really has the new exposure (no leftover brightness), and that the exposure is restored |

| new | Colour camera (OSC) | With a colour Player One or ASI: the header says colour with the pattern; the live view is in colour with plausible colours (pattern right: the Sun is not blue); a SER opens in AutoStakkert/Siril as the right Bayer pattern; ASI colour path has never run on hardware |

| new | Focus assist | In bright sun with tracking on: switch Focus assist on, turn the focuser through focus and back: the score should rise to a clear peak and the best line should match what you judge as sharp; check that it is not fooled by seeing or by drift of the Sun, and whether 1 s bursts are the right length |

## M2: UI

| Issue | Check |
|---|---|
| #50 | Home Screen app on the iPhone: safe areas, sunlight legibility, pad usable with a thumb, overlays draggable, opacity slider |
| #69 | Record with the cap on (dark) and on the Sun; Record > Captures > *Check all* flags the dark one; delete both |
| #63 | Second full UI review on the phone after the overlay changes |

## M3 (last)

| Issue | Check |
|---|---|
| #30 | ASI174MM Mini on a USB 3 cable and blue port: open, set exposure/gain, fps at full frame and ROI |
