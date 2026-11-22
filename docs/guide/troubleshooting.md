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

### Solar tracking is off after a restart

On purpose: RustFlare never switches the mount's tracking on by itself. Use Go to Sun or tick *Solar tracking*.

## Tracking

- *No Sun found*: too dark (raise the exposure) or no limb in view. Use the anchor tracker on a sunspot or plage.
- *Target lost*: the tracker keeps retrying; guiding pauses until it recovers.
- The Sun's measured size disagrees with Optics: check the focal length and Barlow factor, then use *Use measured*.

## Recording and storage

- *Storage not available*: the drive is not mounted. Reconnect it or choose another in More > Storage.
- **Disk full**: recording stops cleanly below 50 MB free, keeping the frames, and will not start below 200 MB. Free up space or pick another drive.
- **The drive cannot be written**: after `sudo apt install` the service runs as the installing user. If another user owns the drive, run `sudo rustflare-user <your-login>`.
- **The recording stops at 4 GB**: not a stop. FAT32 sticks cannot hold bigger files, so the recording continues in `_part2`, `_part3` files. exFAT or ext4 avoids the split (RustFlare never formats a drive).
- **Mostly dark** in the file manager: the cap was on or the Sun was out of view.

## Clock and site

The Pi computes where the Sun is from its clock and your site. Without a network the clock may be wrong, and Go to Sun then points the wrong way. More > Site shows whether the clock is synchronised. A Pi with no network and no battery-backed clock needs a network connection at least once to set it.

## The page will not load

- Use `http://<pi-name>.local:8080`, or the Pi's IP address if `.local` names do not work on your network.
- After enabling HTTPS, use port 8443 and trust the certificate: see [Phone setup](phone-setup.md).
- On the Pi: `systemctl status rustflare` and `journalctl -u rustflare -f` show the service and its log.

## Location button does nothing

Browsers share location only over HTTPS. Set up [HTTPS](phone-setup.md), or type the coordinates.
