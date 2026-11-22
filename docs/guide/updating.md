# Updating

An update restarts the service, so do not update in the middle of an observation. Your settings and captures are kept.

## With rustflare-update

```sh
sudo rustflare-update            # install the newest release (verifies its SHA-256)
rustflare-update --check         # only report
```

To check daily: `sudo systemctl enable --now rustflare-update.timer` (off by default). If the repository is private, put a read token in `/etc/default/rustflare` (`RUSTFLARE_GITHUB_TOKEN`).

## With the signed apt repository

With the repository, the Pi installs and upgrades RustFlare with plain `apt`, and every package is checked against the project's signature. Set it up once on each Pi:

```sh
curl -fsSL https://ddompe.github.io/rustflare/rustflare.gpg | sudo tee /usr/share/keyrings/rustflare.gpg >/dev/null
echo "deb [signed-by=/usr/share/keyrings/rustflare.gpg arch=arm64] https://ddompe.github.io/rustflare stable main" | sudo tee /etc/apt/sources.list.d/rustflare.list
sudo apt update && sudo apt install rustflare
```

After that, `sudo apt update && sudo apt upgrade` updates RustFlare along with the system, and `rustflare-update` keeps working too.

## Updating from a script

`rustflare-update` keeps your edited `/etc/default/rustflare` automatically. With plain `dpkg -i` from a script or ssh (no terminal), dpkg stops at its "configuration file modified" question and the upgrade is left half-done. Use `sudo dpkg --force-confold -i rustflare_*.deb` instead (or `sudo apt install ./rustflare_*.deb` in a terminal, where you can answer N).

## Checking the version

[More > Updates](more.md#updates) shows the installed version, or run `dpkg -s rustflare`.
