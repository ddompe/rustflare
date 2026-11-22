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

- **Record position** saves where the mount points, where the Sun sits in the picture and the time.
- **Flip and re-centre** records, slews the mount back to that pointing (the mount turns over if it decides to), switches solar tracking on again, then brings the Sun back to its recorded place in the picture. It allows for the image turning 180 degrees by checking where the Sun really appears.
- **Restore after a manual flip** does the same after you flipped with another app.

A [guider calibration](track.md#calibrate) is required for the re-centring. How far the mount can track past the meridian is the mount's own setting; change it in the ZWO app.

!!! note "Status"
    Meridian flip assist is built and tested on the simulator; it has not yet been run on a real mount.
