# Aim tab

![Aim tab](../img/manual/aim.png)

## Connect the mount

Choose USB or Bluetooth and connect. For Bluetooth, close the ZWO phone app first and press the mount's pairing button, then use *Scan for mounts*. The state tags show the link and what the mount reports. See [Troubleshooting](troubleshooting.md#mount-bluetooth-does-not-connect).

## Go to Sun

**Go to Sun** sends your site and the time to the mount, slews to where the Sun is now and then switches on solar tracking by itself. It asks for confirmation, and **STOP** cancels. It needs:

- your site set in [More > Site](more.md#site),
- the Sun above the horizon,
- a Pi clock that is synchronised (it warns you if not).

**Send home** goes to the mount's home position, with confirmation.

## Move by hand

The N/S/E/W pad moves the mount while held; release to stop. Arrow keys work too. The slew speed is remembered. **STOP** always cancels motion. The *Solar tracking* switch follows what the mount itself reports.

## Meridian flip

![Meridian flip](../img/manual/meridian-flip.png)

Aim tab > Meridian flip. The panel shows how long until the Sun crosses the meridian (it needs the site set).

**Flip automatically** does it for you. About 5 minutes before the meridian it records where the mount points and where the Sun sits in the picture; about 1 minute after the meridian (or as soon as the mount stops tracking by itself) it:

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
