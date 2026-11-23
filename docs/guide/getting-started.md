# Getting started

RustFlare runs on a Raspberry Pi at the telescope. You install it once, then control it from a phone or computer browser on the same network.

## What you need

- A Raspberry Pi 4 or 5 with Raspberry Pi OS (64-bit, Bookworm or newer), on your network.
- A supported camera and, optionally, a mount (see [Hardware](../reference/hardware.md)). You can try everything without hardware, see [below](#try-it-without-hardware).
- A phone, tablet or computer on the same Wi-Fi (or the Pi's own [hotspot](hotspot.md)).

## Install on the Pi

Download the package from the [latest release](https://github.com/ddompe/rustflare/releases/latest) and install it:

```sh
sudo apt install ./rustflare_*_arm64.deb
```

That is the whole install. For a Pi that will run unattended at the telescope, follow [Setting up the Pi](pi-setup.md) once (about ten minutes) and finish with `rustflare-check`. It creates the service, starts it and enables it at boot. To get updates through `apt` instead, set up the [apt repository](updating.md#with-the-signed-apt-repository).

## First connection from the phone

1. Join the same Wi-Fi as the Pi.
2. Open `http://<pi-name>.local:8080/` in the browser (`<pi-name>` is the Pi's host name).
3. Optional but recommended: set up [HTTPS and a password](phone-setup.md), and add the page to the Home Screen.

!!! warning
    By default there is no password and the server can move your mount. Keep it on a trusted network, or set a password.

The screen has the live view on top and five tabs along the bottom: **Camera, Aim, Track, Record, More**. They follow a session. Start with the [tour of the screen](screen.md).

## Before the first night

Do these once, at home (all in the **More** tab, see [More](more.md)):

- **Site**: enter your latitude and longitude.
- **Optics**: enter the telescope's focal length and any Barlow or reducer.
- **Storage**: choose the USB drive for captures.

## What the service does by itself

The service starts at boot; there is nothing to launch. It reconnects the last camera and mount automatically, retrying for a couple of minutes while USB devices come up. Untick *reconnect camera & mount* in the Camera tab to turn that off. Closing the camera or disconnecting the mount on purpose makes it forget them.

It never switches on the mount's tracking by itself, because that moves the mount. After a restart, use **Go to Sun** or tick *Solar tracking* again. Tracking targets and the guider calibration are not remembered between sessions.

## Try it without hardware

A simulated camera and mount are always listed. Open the simulated camera, connect the simulator mount, switch on solar tracking, start tracking, calibrate and centre. *Sim disc size* (Camera tab) above about 1.6 makes the disc overflow the frame like an H-alpha view. To make Optics agree with it, enter a focal length of 725 mm for a disc size of 3.

## Remove

`sudo apt remove rustflare` keeps your captures and the service account. `sudo apt purge rustflare` also removes the configuration files.

## Next

[The screen](screen.md), then the tabs in order: [Camera](camera.md), [Aim](aim.md), [Track](track.md), [Record](record.md).
