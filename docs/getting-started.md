# Getting started

## Install on a Raspberry Pi

Requires Raspberry Pi OS (64-bit, bookworm or newer) on a Pi 4/5. From the [latest release](https://github.com/ddompe/rustflare/releases/latest):

```sh
sudo apt install ./rustflare_*_arm64.deb
```

That is the whole install. It creates the service, starts it, and enables it at boot. Open `http://<pi-name>.local:8080/` from any machine on the same network.

| | |
|---|---|
| Web UI | `http://<pi-name>.local:8080/` |
| Service | `systemctl status rustflare`, logs: `journalctl -u rustflare -f` |
| Settings | `/etc/default/rustflare` (listen address, default capture folder, update repo) |
| Files | program: `/opt/rustflare`; captures: `/var/lib/rustflare/captures` or the drive you choose; saved UI settings: `/var/lib/rustflare/rustflare.json` |

!!! warning
    There is **no authentication** and the server can move your mount. Keep it on a trusted network.

### Update

```sh
sudo rustflare-update            # install the newest release (verifies its SHA-256)
rustflare-update --check         # only report
```

Config and captures are kept. If the repository is private, put a read token in `/etc/default/rustflare` (`RUSTFLARE_GITHUB_TOKEN`). To check daily: `sudo systemctl enable --now rustflare-update.timer` (off by default).

### Remove

`sudo apt remove rustflare` keeps your captures and the service account. `sudo apt purge rustflare` also removes config files.

## Storage: use a USB drive

Open **Storage** in the web UI, pick the drive, press *Use*. The choice is remembered. See [Capture](capture.md#storage) for what Rustflare does when a drive is removed, full, or FAT32.

The installer runs the service as the user who ran `sudo apt install` so that drives auto-mounted for that login are writable. Check or change it with `rustflare-user` / `sudo rustflare-user <login>`.

## Try it without hardware

The simulated camera and mount are always available in the UI. Open the simulated camera, connect the simulator mount, switch on solar tracking, start tracking, calibrate, centre. *Sim disc size* (Camera panel) above ~1.6 makes the disc overflow the frame like an H-alpha view; to make the Optics panel agree with it, enter focal length 725 mm for a disc size of 3.

## Run from source (development)

See [Development](development.md). In short: `cd web && npm install && npm run build && cd .. && cargo run --release -p rustflare-server`.
