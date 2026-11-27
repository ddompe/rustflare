# Troubleshooting and FAQ

A red banner at the top of the screen shows the last problem. Find it below.

## Camera

### The camera is not found

- Check the USB cable and port, then look at the camera list in the Camera tab. A Player One or ZWO ASI camera appears by its name.
- ZWO ASI cameras need the `libasi` package: `sudo apt install libasi`. Without it no ASI camera is listed.
- The simulated camera is always there. If only that appears, the real camera is not visible to the Pi (`lsusb` on the Pi shows whether it is).

### The frame rate is low

A USB 3 camera on a USB 2 port or cable runs at a fraction of its speed (about 9 to 17 fps instead of 50 or more). Use a blue (USB 3) port and a USB 3 cable. Put the USB stick on a different blue port than the camera so they do not share a controller.

## Mount

### Mount Bluetooth does not connect

Close the ZWO phone app first (the mount accepts only one connection), press the mount's pairing button, then use *Scan for mounts* again. If the link drops, press the pairing button again. A USB mount is found again automatically.

### Go to Sun refuses

Set your site (More > Site), check that the Sun is above the horizon, and check the clock (below). It also refuses while a recording is running.

### Tracking is off after a restart

On purpose: RustFlare never switches the mount's tracking on by itself. Use Go to Sun (or Go to a planet or the Moon) or tick the tracking switch on the Aim tab; it keeps the rate you last used (solar, lunar or star).

## Tracking

- *No Sun found*: too dark (raise the exposure) or no limb in view. Use the anchor tracker on a sunspot or plage.
- *Target lost*: the tracker keeps retrying; guiding pauses until it recovers.
- The Sun's measured size disagrees with Optics: check the focal length and Barlow factor, then use *Use measured*.

### The Sun jumps, or tracking is lost in shadows and cloud

Brief losses (under 2 seconds) are ignored. Longer ones show in the status bar; guiding goes on from the measured drift if it can, and resumes by itself. If the Sun really moved a long way and came back, read the mount log: see [Debugging and logs](../reference/debugging.md#reading-them).

### The mount stopped tracking past the meridian

The AM3 stops tracking at its own limit past the meridian (about 19 minutes on 2026-11-22 with the flip off). Flip before then: [Meridian flip](aim.md#meridian-flip). A *Go to Sun* sent afterwards makes the mount turn over, and the Sun then has to be centred by hand.

## Recording and storage

- *Storage not available*: the drive is not mounted. Reconnect it or choose another in More > Storage.
- **Disk full**: recording stops cleanly below 50 MB free, keeping the frames, and will not start below 200 MB. Free up space or pick another drive.
- **The drive cannot be written**: after `sudo apt install` the service runs as the installing user. If another user owns the drive, run `sudo rustflare-user <your-login>`.
- **The recording stops at 4 GB**: not a stop. FAT32 sticks cannot hold bigger files, so the recording continues in `_part2`, `_part3` files. exFAT or ext4 avoids the split (RustFlare never formats a drive).
- **Mostly dark** in the file manager: the cap was on or the Sun was out of view.

## Stacking from memory

**It says "bursts are recorded as video and not stacked from memory".** The stacker was not running or not connected, a burst was too big for the RAM buffer, or a stack could not finish in time. The message in the status bar gives the reason; nothing was lost. Check `systemctl status rustflare-stack`, and More > RAM buffer.

**"Stack only" is refused.** It needs the stacker, and there is no stacker connected: `sudo systemctl start rustflare-stack`.

**A stack was made but I wanted the video.** Choose *Keep video* or *Video + stack* before recording. A burst that was not written cannot be recovered.

## Clock and site

The Pi computes where the Sun is from its clock and your site. Without a network the clock may be wrong, and Go to Sun then points the wrong way. More > Site shows whether the clock is synchronised. A Pi with no network and no battery-backed clock needs a network connection at least once to set it.

## The page will not load

- Use `http://<pi-name>.local:8080`, or the Pi's IP address if `.local` names do not work on your network.
- After enabling HTTPS, use port 8443 and trust the certificate: see [Phone setup](phone-setup.md).
- On the Pi: `systemctl status rustflare` and `journalctl -u rustflare -f` show the service and its log.

## Location button does nothing

Browsers share location only over HTTPS. Set up [HTTPS](phone-setup.md), or type the coordinates.

## The camera or the drive keeps dropping off

Signs: "camera error" now and then, a recording that stops, guiding that stops by itself, `usb 2-1: USB disconnect` in `journalctl -k`, a drive that comes back read-only. On the Raspberry Pi 5 the first thing to check is the ports: **the capture drive belongs on a black USB 2.0 port**, the camera on a blue USB 3 port. With both on USB 3 they dropped repeatedly under load; with the drive on USB 2.0 they did not (see [Pi setup](pi-setup.md#1-the-capture-drive)). The Storage panel shows a warning when it sees that arrangement. Next, power: `vcgencmd get_throttled` should say `0x0` and `journalctl -k | grep -i undervolt` should be empty; a weak supply or a long thin cable causes the same symptoms.

## The drive was pulled without ejecting

Pulling the USB drive during a recording stops the recording cleanly and keeps what was already written (the SER header is updated every 50 frames, so the file opens; `scripts/ser-repair.py` repairs a file that was cut earlier). The frames still in memory are lost, and the app says so until you dismiss it. Afterwards More > Storage names the problem in words:

- **"was unplugged without being ejected"**: the mount is left over from the removed drive. Plug the drive in again; if it appears under a new name the app finds it by its label, otherwise choose it in More > Storage.
- **"is mounted read-only: the filesystem has errors"**: the kernel found errors in the drive's filesystem, which is what a pulled drive comes back as. Nothing can be recorded to it until it is checked: unmount it and run `sudo fsck.vfat -a /dev/sdX1` on the Pi (the message gives the device), then plug it in again.

Use **Eject drive safely** (More > Storage) to avoid all of this.
