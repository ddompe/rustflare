# Aim tab

![Aim tab](../img/manual/aim.png)

!!! note "Screenshot"
    The picture is from 2026-11-21 and predates the driver choice, *Find my mount*, *Mount time and place* and the Go to selector. The text is current.

## Connect the mount

Choose the **driver**: *zwo* (the AM3 and AM5 family), *ioptron* (CEM70 series and relatives, the HAE69C) or *simulator*.

- **ZWO:** choose USB cable or Bluetooth. For Bluetooth, close the ZWO phone app first and press the mount's pairing button, then use *Scan for mounts* and pick the mount in the list. See [Troubleshooting](troubleshooting.md#mount-bluetooth-does-not-connect).
- **USB (either brand):** press **Find my mount**. It asks each serial port a read-only question of each protocol and names the model that answered (a ZWO mount, or an iOptron one such as `iOptron HAE69C`); pick it and connect. If nothing answers, switch the mount on, check the cable, and pick the driver by hand. A device path can also be typed.
- **iOptron:** connect the mount's USB port (or its RS-232 port through an adapter) to the Pi. The 115200 baud setting is applied for you. The HAE69C was seen to answer some commands (tracking, stop) in 300 to 600 ms, so the driver waits up to 2 seconds for a reply.

The state tags show the link and what the mount reports.

## Mount time and place

Go to is computed from the site and the time the **mount** holds, not only from the Pi's. Open **Mount time and place** to see what the mount says next to the Pi's, with a one-line summary in exact words, for example "time matches · place not reported" or "time differs by 3 min · place differs by 1100 km" (the clock may differ by 5 seconds and the place by 0.05 degrees; a mount that does not report its place is never called a match). Times are shown in the Pi's local time first and UTC second. When something that can be compared differs, it is one line in the notes chip on the picture, with **Send now** and **Show on Aim** (which opens this section). It is a note, not a warning: Go to sends the Pi's site and time itself first, and its confirmation says so. **Send site and time to the mount** sends the Pi's saved site and clock; it moves nothing, and it is greyed with the reason when no site is saved (tap *Use my location* or set it in More > Site), the mount is parked or it is moving. The Pi's own clock matters first: the block says when it is not synchronised to a time server, and then the Pi should be fixed before anything is sent. A mount that cannot report its site and time shows the block greyed, and a ZWO mount reports its clock only. The switch **Send them by themselves when connecting** (on by default) makes the Pi send its site and time once after connecting, when they differ, the Pi's own clock is synchronised, a site is saved and the mount is not moving. It is for mounts without a clock battery: the iOptron HAE69C restarts from 2017-01-01 14:00 UTC every time it is switched on.

## Go to Sun

**Go to Sun** sends your site and the time to the mount, slews to where the Sun is now and then switches on solar tracking by itself. It asks for confirmation, and **STOP** cancels. It needs:

- your site set in [More > Site](more.md#site),
- the Sun above the horizon,
- a Pi clock that is synchronised (it warns you if not).

### The Moon and the bright planets

The selector above the button lists the Sun, Moon, Mars, Jupiter and Saturn with their altitude now ("Saturn · 34° up", or "below horizon"). Pick one and the button reads *Go to Saturn*; the rest is the same: confirmation, **STOP** cancels, and tracking starts on arrival at the **lunar** rate for the Moon and the **sidereal** rate for planets (the Sun gets the solar rate). The selector always starts at the Sun, so the button cannot surprise you.

- **Below the horizon:** the target is marked *not up (below the horizon)* and the button is greyed with the reason; the screen never sends the mount below the horizon (the HTTP API can, for the Moon and planets, with `allow_below_horizon`). Under 15 degrees it warns about trees, buildings and poor seeing.
- **Accuracy:** positions are computed on the Pi (planets from approximate orbital elements, the Moon from a short series with parallax for your site), good to a few arcminutes. That puts the target in the field of a narrow camera; centre it by hand with the N/S/E/W pad.
- The Moon is large: with a long focal length you see only part of it, so aim for the limb or a crater and use short exposures.

**Send home** goes to the mount's home (zero) position, with confirmation. Mounts whose driver can do it also show **Search home...** (the iOptron): the mount moves both axes until its home sensors find the mechanical zero and **makes that its new zero**. It is not the same as Send home; use it only when the zero is wrong, with someone at the mount.

## Move by hand

The N/S/E/W pad moves the mount while held; release to stop. Arrow keys work too. The **Slew speed** list comes from the mount's driver (a ZWO mount offers its own speeds; an iOptron offers 1x to 512x sidereal and its maximum, which depends on the model) and the choice is remembered. **STOP** always cancels motion. The tracking switch follows what the mount itself reports, and its label says the rate (*Solar*, *Lunar* or *Star*). Three buttons under it choose the rate. A Go to leaves the rate of its target on, and switching tracking off and on again keeps it; only an explicit choice (or Go to Sun, or a meridian flip, which is for the Sun) changes it.

## Meridian flip

![Meridian flip](../img/manual/meridian-flip.png)

Aim tab > Meridian flip. The panel shows how long until the Sun crosses the meridian (it needs the site set).

**Flip automatically** does it for you. About 5 minutes before the meridian it records where the mount points and where the Sun sits in the picture; about 1 minute after the meridian (or as soon as the mount stops tracking by itself) it:

**Switch it on before the meridian.** It only acts when it recorded the position beforehand. If you switch it on after the Sun has crossed (more than the flip delay past the meridian), it says so and does nothing until the next crossing, because the mount cannot tell which side of the pier it is on: a mount you already flipped by hand would be turned back, and the Sun lost.

1. **pauses a running recording** (it resumes by itself after a clean flip),
2. sends the mount to the recorded pointing, which turns it over,
3. switches solar tracking on and brings the Sun back to its recorded place, allowing for the image turning 180 degrees,
4. recalibrates the guider if it was guiding, and goes back to guiding.

The status bar and the **Meridian flip** overlay say which step it is on. Both timings can be changed under the switch. If anything goes wrong the recording is *not* resumed and the status bar says why.

Without the automatic mode:

- **Record position** saves the pointing, the Sun's place and the time.
- **Flip and re-centre** does the same steps as above on demand.
- **Restore after a manual flip** does the re-centring after you flipped with another app.

A [guider calibration](track.md#calibrate) is needed for the re-centring. How far the mount keeps tracking past the meridian is the mount's own setting (ZWO app). If it stops, it stops tracking at its limit and the Sun drifts away, so flip before that. A plain *Go to Sun* sent after the meridian also makes the mount turn over; expect to centre the Sun by hand afterwards, because the mount's pointing is less accurate on the other side. The image then turns by about 180 degrees: recalibrate.

!!! note "Status"
    Built and tested on the simulator. The first real flip is still to be done (see the [hardware checklist](../project/hardware-checklist.md)). On 2026-11-22 the mount turned over on a Go to Sun after it had stopped at its limit, and the calibration afterwards showed the image rotated by about 209 degrees.
