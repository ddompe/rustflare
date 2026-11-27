# More tab

![More tab](../img/manual/more.png)

Settings you change rarely.

## Site

Type your latitude and longitude in degrees, east positive (a site west of Greenwich has a negative longitude), and save. It stays on the Pi and is used by **Go to Sun**. The section also shows whether the Pi's clock is synchronised to a time server, which Go to Sun needs.

*Use this device's location* works only over HTTPS (browsers share location only on secure pages), see [Phone setup](phone-setup.md).

## Optics

Enter the telescope's native focal length and, if you use one, the Barlow or reducer factor. RustFlare shows the effective focal length, the image scale and how large the Sun should be in pixels, and tells you when the measured Sun disagrees.

### Optical setups (your telescopes)

If you use more than one telescope, or one telescope with several reducers and Barlows, save each as a **named optical setup** (for example *Lunt LS50 + Barlow* for the Sun in H-alpha, *C11 + reducer* for the Moon and planets). A setup holds the focal length, the Barlow or reducer factor, the pixel size, the binning, an optional aperture (it shows the f-ratio) and notes, **the profile that train is used for**, and the master flat and dark that were in use when it was saved. Nothing is created for you: an install that never saves a setup behaves as before.

- **Save as...**: type a name (1 to 40 characters) and press it. What is typed in the optics fields is put in use first, the setup is saved with the *Profile of this train* chosen above it (the current profile to begin with, or *Keep the current profile*), and it becomes the setup in use.
- **Apply**: pick a setup and press Apply. The optics and the profile change together (so the expected size and the tracker follow), and its flat and dark are selected again. The result lists what changed: profile, effective focal length, image scale in arcseconds per pixel and the expected size in pixels. A flat or dark that is gone is reported and everything else is still applied. Applying never moves the mount and never starts a recording or tracking.
- **Modified**: if you change the optics or the profile after applying a setup, it is marked *modified*. Nothing is overwritten: press **Revert** to go back to the saved setup, **Update** to save the changes into it, or **Save as...** under another name.
- **Duplicate** copies the picked setup under a new name. **Delete...** asks first and says what is forgotten; the optics in use stay as they are.

The name of the setup in use is in the Optics summary, on the Camera tab and under the profile picker.

## Storage

The drive is remembered by its **filesystem UUID**, not by the name it was mounted under. Desktop auto-mounting names a drive after its label and appends a number when a stale mount point is left (`ASTRODRIVE`, then `ASTRODRIVE1`); RustFlare finds the drive again wherever it is mounted, and settings saved by older versions follow a renamed mount by themselves when exactly one mount fits.

Pick the drive for captures (a USB stick appears automatically) and press *Use*; the choice is remembered. It shows free space and the recording time left at the current frame rate.

- A removed drive is never silently replaced by the SD card: recording refuses to start until it is back.
- FAT32 sticks work, but a file cannot exceed 4 GB, so long recordings continue in `_part2`, `_part3` files.
- Use a USB 3 (blue) port for the stick and a different blue port for a USB 3 camera, so they do not share a controller.
- **Eject drive safely** flushes and unmounts the stick (and powers it off) before you pull it.

## Display

Stretch, brightness, contrast, gamma, tint, bullseye and the overlays. See [The screen](screen.md#overlays).

## Guiding settings

How strongly the guider corrects. See [Track](track.md).

## Session log

An opt-in log of what the tracker and guider saw, plus small frames. Useful when reporting a problem. It writes into a `logs` folder on the capture drive and stops at 200 MB. Nothing identifying is logged.

## Updates

Shows the installed version and whether a newer one exists. See [Updating](updating.md).
