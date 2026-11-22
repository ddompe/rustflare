# 0017. No updater: debs and the signed apt repository

- Status: Accepted
- Date: 2026-11-22

## Context

[ADR 0010](0010-debian-package-and-updates.md) added `rustflare-update`, a script (with a systemd service and timer) that read the latest GitHub release, verified its SHA-256 from the same release and installed the package. It was one more program to maintain and to document, it duplicated what `apt` does, and a checksum published next to the file it protects does not defend against a compromised release. The signed apt repository ([apt-repository.md](../project/apt-repository.md)) already gives `apt upgrade` with signature verification.

## Decision

Remove `rustflare-update`, its service and timer, the `RUSTFLARE_REPO` and `RUSTFLARE_GITHUB_TOKEN` settings and the mock release server. RustFlare is distributed as:

- the arm64 `.deb` and `SHA256SUMS` attached to each GitHub release (install with `sudo apt install ./file.deb`), and
- the signed apt repository (`apt upgrade`).

Nothing on the Pi checks for or installs updates by itself.

## Consequences

- Less to maintain and nothing running on a timer on an observing machine.
- Pis that had the updater lose the command when they install the next package (the package no longer ships it); an enabled `rustflare-update.timer` goes with it.
- Updating needs a person or `apt upgrade`. Development builds still go through `scripts/pi-deploy.sh`.
