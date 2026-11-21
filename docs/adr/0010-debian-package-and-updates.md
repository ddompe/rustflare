# 0010. Debian package, GitHub Releases, and `rustflare-update`

- Status: Accepted. Package, install, upgrade and the update command are verified on a Raspberry Pi 5 (Debian 13); the CI workflows have not yet run on GitHub.
- Date: 2026-11-21

## Context

The target is a headless Raspberry Pi at a telescope. Installing must be one command, updates must be simple and safe,
and the Pi should not need a Rust or Node toolchain (a Pi 5 compiles the workspace in minutes, but that is the wrong thing to ask of an appliance).

## Decision

- **Format: a `.deb`** built with `cargo-deb`. It installs the binary, the bundled Player One library and the web UI under `/opt/rustflare`, a systemd unit, a udev rule, `/etc/default/rustflare`, and two commands (`rustflare-update`, `rustflare-user`). Dependencies (`libusb-1.0-0`, `python3`, `curl`) come from apt. Config files are conffiles, so upgrades keep local edits; captures live in `/var/lib/rustflare` (or the chosen drive) and are never removed, even on purge.
- **Cross-compiled, not built on the Pi**: `cargo-zigbuild` (Zig as the C linker) targeting `aarch64-unknown-linux-gnu.2.36` (Raspberry Pi OS bookworm and newer). The simulator-only build takes ~17 s on a laptop. `scripts/build-deb.sh` does the whole thing, locally and in CI.
- **Distribution: GitHub Releases.** Pushing a tag `vX.Y.Z` (which must equal the Cargo version) runs `release.yml`: build, then publish `rustflare_X.Y.Z-1_arm64.deb` and `SHA256SUMS`. `ci.yml` builds the package on every push so packaging cannot rot unnoticed.
- **Updates: `rustflare-update`** reads the latest release via the GitHub API, takes the version from the package file name (the same string `dpkg` compares), verifies the SHA-256, and runs `apt-get install`. It works for private repositories with a token in `/etc/default/rustflare`. A daily `rustflare-update.timer` is shipped but **not enabled**: unattended updates of a thing that moves a mount are the user's call. `RUSTFLARE_API_URL` redirects it (used with `scripts/mock-github-release.py` to test the whole flow without GitHub).
- **Service account.** The package creates a `rustflare` system user. On a first install via `sudo apt install`, the service is switched (systemd drop-in) to the person who ran sudo, because USB drives auto-mounted by the desktop belong to that login and a system account cannot write to them (see [ADR 0011](0011-storage-selection-and-safety.md)). `rustflare-user` shows or changes this at any time.

## Alternatives considered

- *`curl | sh` installer that downloads a tarball*: no dependency handling, no clean uninstall or upgrade, no file ownership tracking.
- *Docker*: USB camera and serial passthrough, udev, and a heavier Pi footprint for no gain here.
- *A signed apt repository* (GitHub Pages + `reprepro`): the right long-term answer (`apt upgrade` just works, signature verification). It needs a signing key that the project owner should create and guard, so it is deferred; releases + `rustflare-update` give the same install/upgrade UX meanwhile. SHA-256 from the same release protects against corruption but **not** against a compromised release; a signature would.
- *Building on the Pi*: slow, needs toolchains there, and makes every update a build.

## Consequences

- Install is `sudo apt install ./rustflare_*.deb`; update is `sudo rustflare-update`; remove is `sudo apt remove rustflare`.
- The bundled vendor library (`playerone-sdk-sys`) is closed source and redistributed inside the package; check its terms before publishing packages publicly (see [ADR 0006](0006-playerone-via-vendor-sdk.md)).
- Only arm64 is packaged. An amd64 package would be a second matrix entry.
