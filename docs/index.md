# RustFlare

RustFlare is headless solar capture for a Raspberry Pi. It runs next to your telescope and you control it from the web browser on your phone: live view, Go to Sun, tracking and guiding, and recording in SER, FITS or PNG.

This site has three parts.

## 1. User guide

For the person using the app at the telescope. Task by task, in plain language, with screenshots.

- [Getting started](guide/getting-started.md): install on the Pi and connect from the phone
- [The screen](guide/screen.md): the live view, overlays and **Hide controls**
- The tabs: [Camera](guide/camera.md), [Aim](guide/aim.md), [Track](guide/track.md), [Record](guide/record.md), [More](guide/more.md)
- [Phone setup](guide/phone-setup.md): HTTPS, password, Home Screen app
- [Field networking](guide/hotspot.md): the Pi as a Wi-Fi hotspot
- [Updating](guide/updating.md)
- [Troubleshooting and FAQ](guide/troubleshooting.md)

## 2. Reference

Look-up material: what hardware works, what the files contain, settings and numbers.

- [Hardware](reference/hardware.md): supported cameras and mounts
- [Capture formats and storage](reference/capture-formats.md)
- [Tracking and guiding concepts](reference/tracking-and-guiding.md)
- [Configuration](reference/configuration.md): settings and environment files
- [HTTP API](reference/api.md)
- [Performance](reference/performance.md)

## 3. Developer and project

For people working on RustFlare itself. You do not need any of this to use it.

- [Architecture](project/architecture.md), [Development guide](project/development.md)
- [Decisions (ADRs)](adr/README.md)
- [ZWO mount Bluetooth protocol notes](project/zwo-ble-protocol.md)
- [Roadmap and status](project/roadmap.md), [Next-round plan](project/next-round.md)
- [Hardware verification checklist](project/hardware-checklist.md)
- [Performance audit](project/performance-audit.md), [UI review](project/ui-review.md)
- [Maintainer setup of the apt repository](project/apt-repository.md)
- [Brand](project/brand.md)
