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
