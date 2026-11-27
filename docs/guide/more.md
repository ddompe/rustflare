# More tab

![More tab](../img/manual/more.png)

!!! note "Screenshot"
    The picture is from 2026-11-21. The tab now has these sections, each a line that opens: Display, Optical setup, Site, Stacking, Storage, RAM buffer, Session log and Startup. The text below is current.

Settings you change rarely. The section you opened last stays open.

## Site

Type your latitude and longitude in degrees, east positive (a site west of Greenwich has a negative longitude), and save. It stays on the Pi and is used by **Go to Sun**. The section also shows whether the Pi's clock is synchronised to a time server, which Go to Sun needs.

*Use this device's location* works only over HTTPS (browsers share location only on secure pages), see [Phone setup](phone-setup.md). On a plain `http://` page the button is not offered: type the latitude and longitude instead, and a line says why.

## Optical setup

Choose the optical setup (below), or make one with the telescope's native focal length and, if you use one, the Barlow or reducer factor. RustFlare shows the effective focal length, the image scale and how large the Sun should be in pixels, and tells you when the measured Sun disagrees.

### Optical setup (your telescopes)

If you use more than one telescope, or one telescope with several reducers and Barlows, save each as a **named optical setup** (for example *Lunt LS50 + Barlow* for the Sun in H-alpha, *C11 + reducer* for the Moon and planets). A setup holds the focal length, the Barlow or reducer factor, the pixel size, the binning, an optional aperture (it shows the f-ratio) and notes, **the profile it is used for**, and the master flat and dark that were in use when it was saved. Nothing is created for you: an install that never saves a setup behaves as before.

The page has three things and nothing else:

- **Optical setup**, a dropdown of your saved setups. Choosing one **applies it at once**: the optics and the profile change together (so the expected size and the tracker follow), and its flat and dark are selected again. One line says what you now have, for example *“Lunt + Barlow” applied: 800 mm · 0.97″/px · Sun, H-alpha*. A flat or dark that is gone is reported there and everything else is still applied. Applying never moves the mount and never starts a recording or tracking. While a recording runs the dropdown is greyed with the reason under it, because the optics and the profile are written into the files being recorded.
- **Manage...**, which opens the *Optical setups* dialog (below).
- A read-only line of what is in use: *800 mm (400 × 2) · f/16 · 0.97″/px · expected Sun radius 540 px · Sun, H-alpha*.

When the optics in use match no saved setup the dropdown shows **Custom (not saved)** first. If you change the optics or the profile after applying a setup, the dropdown shows *name (modified)* with a small **Revert** link that goes back to the saved values. Nothing is overwritten by itself.

**The Optical setups dialog** (a full-screen sheet on a phone, a centred panel on a computer; Escape or the close button leaves it and puts you back on Manage...; in the New or Edit form, Escape does nothing while you have typed something, and says so, so that a slip of the key does not lose it: use Save, or Cancel to drop the changes) lists the saved setups with their values and profile and marks the one in use. Each row has:

- **Edit**, which opens the one form below.
- **Duplicate**, which adds a copy named *name copy*; Edit renames it.
- **Delete**, which asks in the row (*Delete “name”? Its values, profile, flat and dark names are forgotten.* Cancel, then Delete). The values in use stay as they are.

**New setup...** opens the same form, filled from the optics in use, so *Custom (not saved)* can be kept under a name in one step. The form has the name (1 to 40 characters, unique, no slash), focal length, Barlow or reducer factor, pixel size override (blank: the camera's), binning, aperture (the f-ratio shows as you type), the profile for this setup (*Keep the current profile* leaves the profile alone), notes, and shows the image scale and expected radius it gives. It also shows the flat and dark that are stored with it; *Include the current flat and dark* is on by default. Problems are written under the field. **Save** keeps it (renaming it in place when you changed the name; if it is the setup in use, the new values are applied too), **Save and use** (new setups) keeps it and applies it. If you just want values in use without keeping them, make a setup called *Custom*.

Nothing is created for you: an install that never saves a setup behaves as before. The setup in use is shown as *In use: name* on the Camera tab (tap it to open the dialog) and under the profile picker. A greyed control always has its reason written under it.

## Storage

The drive is remembered by its **filesystem UUID**, not by the name it was mounted under. Desktop auto-mounting names a drive after its label and appends a number when a stale mount point is left (`ASTRODRIVE`, then `ASTRODRIVE1`); RustFlare finds the drive again wherever it is mounted, and settings saved by older versions follow a renamed mount by themselves when exactly one mount fits.

Pick the drive for captures under **Save to** (a USB stick appears automatically) and press *Use*; the choice is remembered. It shows how much is *Used*, the free space and the recording time left at the current frame rate. **Internal storage (SD card)** is in the same list, with a **Keep this much free (GB)** reserve (see [Record](record.md#recording-to-the-pis-own-sd-card)). The switch *Write recordings directly to the drive* uses direct writes (`O_DIRECT`) instead of the page cache; it is for comparing speeds and is refused if the drive does not allow it.

- A removed drive is never silently replaced by the SD card: recording refuses to start until it is back.
- FAT32 sticks work, but a file cannot exceed 4 GB, so long recordings continue in `_part2`, `_part3` files.
- Use a USB 3 (blue) port for the stick and a different blue port for a USB 3 camera, so they do not share a controller.
- **Eject drive safely** flushes and unmounts the stick (and powers it off) before you pull it.

## Display

Stretch, brightness, contrast, gamma, tint, bullseye and the overlays. See [The screen](screen.md#overlays).

## Stacking

The settings of the separate stacker program (on or off, when it runs, how many frames it keeps, a flat and darks, the black disc on prominence stacks) and how many stacks there are. The stacks themselves are looked at and downloaded in **Files**. See [Stacking](stacking.md).

## RAM buffer

The size of the RAM buffer that recordings pass through (default 1 GB, at most 70 % of the free memory), how many frames it holds at the current frame size, and whether the stacker is connected. It cannot be changed while a recording runs.

## Startup

*Reconnect camera and mount at startup* (on by default): the service reopens the last camera and mount after a restart. Mount tracking is never switched on automatically.

Guiding settings (how strongly the guider corrects) are on the [Track](track.md) tab, under *Guiding settings*.

## Session log

An opt-in log of what the tracker and guider saw, plus small frames. Useful when reporting a problem. It writes into a `logs` folder on the capture drive and stops at 200 MB. Nothing identifying is logged.

## Version

There is no Updates section: RustFlare does not update itself. The installed version is shown at the right of the status bar at the bottom of every screen. See [Updating](updating.md).
