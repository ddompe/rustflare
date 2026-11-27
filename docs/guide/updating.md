# Updating

An update restarts the service, so do not update in the middle of an observation. Your settings and captures are kept.

## With a downloaded package

Each release has the Raspberry Pi package and a checksum file on the [releases page](https://github.com/ddompe/rustflare/releases). Download both, check, install:

```sh
sha256sum -c --ignore-missing SHA256SUMS      # the .deb must say OK
sudo apt install ./rustflare_*_arm64.deb      # installs or upgrades
```

There is no update command on the Pi: it only changes when you install a package.

## With the signed apt repository

With the repository, the Pi installs and upgrades RustFlare with plain `apt`, and every package is checked against the project's signature. Set it up once on each Pi:

```sh
curl -fsSL https://ddompe.github.io/rustflare/rustflare.gpg | sudo tee /usr/share/keyrings/rustflare.gpg >/dev/null
echo "deb [signed-by=/usr/share/keyrings/rustflare.gpg arch=arm64] https://ddompe.github.io/rustflare stable main" | sudo tee /etc/apt/sources.list.d/rustflare.list
sudo apt update && sudo apt install rustflare
```

After that, `sudo apt update && sudo apt upgrade` updates RustFlare along with the system.

## Updating from a script

`apt install ./file.deb` keeps your edited `/etc/default/rustflare` automatically. With plain `dpkg -i` from a script or ssh (no terminal), dpkg stops at its "configuration file modified" question and the upgrade is left half-done. Use `sudo dpkg --force-confold -i rustflare_*.deb` instead (or `sudo apt install ./rustflare_*.deb` in a terminal, where you can answer N).

## Checking the version

The installed version is shown at the right of the status bar at the bottom of every screen (a package built on a computer with `scripts/pi-deploy.sh` says `local` and the commit it was built from), or run `dpkg -s rustflare`. See [More](more.md#version).

## Release packages and local builds

A release is made by pushing a tag `vX.Y.Z`; the release workflow builds the arm64 package and publishes it with `SHA256SUMS`. During development a package built from a checkout with `scripts/pi-deploy.sh user@host` (it builds, installs and refuses while a recording runs) carries a version like `0.8.2+dev<date>.<commit>`, which sorts above the release it is based on, so the next release still upgrades it.
