# Configuration

## Where things are

| | |
|---|---|
| Web UI | `http://<pi-name>.local:8080/` (HTTPS on port 8443 after [setup](../guide/phone-setup.md)) |
| Service | `systemctl status rustflare`; log: `journalctl -u rustflare -f` |
| Environment file | `/etc/default/rustflare` |
| Program | `/opt/rustflare` |
| Captures | `/var/lib/rustflare/captures`, or the drive you choose in More > Storage |
| Saved settings | `/var/lib/rustflare/rustflare.json` (site, optics and optical setups, profile, flats and darks, guiding, colour, coronagraph, storage, startup, UI layout) |
| Stacker settings | `/var/lib/rustflare/stack.json` and `stack-status.json`, see [Stacking](../guide/stacking.md) |
| Logs | `/var/lib/rustflare/logs/<date>/` (events, mount) and `logs/<date>/` on the capture drive, see [Debugging](debugging.md) |
| Certificates | `/var/lib/rustflare/tls/` |

## The environment file

`/etc/default/rustflare` is read by the service. Restart after editing: `sudo systemctl restart rustflare`. A package upgrade keeps your edits.

| Variable | Meaning |
|---|---|
| `RUSTFLARE_BIND` | Address and port for plain HTTP (default `0.0.0.0:8080`) |
| `RUSTFLARE_DATA_DIR` | Default capture folder on the SD card. A drive chosen in the UI takes precedence |
| `RUSTFLARE_PASSWORD` | Password for the UI and API (HTTP Basic, any user name). Use with HTTPS |
| `RUSTFLARE_TLS_BIND` | HTTPS address, for example `0.0.0.0:8443`. Set up by `sudo rustflare-tls` |
| `RUSTFLARE_CONFIG`, `RUSTFLARE_TLS_DIR`, `RUSTFLARE_WEB_DIR` | The settings file, the certificate folder and the folder of the web UI (the service runs in `/var/lib/rustflare`, so the defaults `rustflare.json` and `tls` land there; the package sets the web folder) |

## Helper commands

| Command | Purpose |
|---|---|
| `rustflare-tls` | Create the private certificate authority and server certificate, see [Phone setup](../guide/phone-setup.md) |
| `rustflare-hotspot` | Wi-Fi hotspot, see [Field networking](../guide/hotspot.md) |
| `rustflare-user` | Show or change the login the service runs as (`sudo rustflare-user <login>`). It matters because drives auto-mounted for a login are writable only by that login |
