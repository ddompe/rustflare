# 0011. Storage selection and safety

- Status: Accepted. Logic is unit-tested; selection and listing were exercised on the Pi's real FAT32 drive read-only. Writing to the drive from the UI has not been tried on hardware.
- Date: 2026-11-21

## Context

Captures are large (2.4 MB per 1280×960 16-bit frame) and the SD card is small and wears out, so users capture to an attached USB drive. Observed on the target Pi: a 233 GB **FAT32** stick, auto-mounted by the desktop for the login user (`uid=1000`, `fmask=0022`), already holding unrelated data. That exposes four problems: choosing the location, a system service user cannot write to it, FAT32 caps files at 4 GB, and the stick can be unplugged or fill up mid-capture, in which case naive code writes into the now-empty mount-point directory on the SD card.

## Decision

- **Selectable at run time** from the web UI (Storage panel): detected mounts under `/media`, `/mnt`, `/run/media` are listed with filesystem, free space and recording time left at the current frame rate. Choosing one uses a `rustflare/` folder on it. The choice is saved in `rustflare.json` (settings file, one key per feature) and **takes precedence** over `--data-dir` / `RUSTFLARE_DATA_DIR`, which stays the default.
- **Never silently fall back.** The chosen directory remembers which mount point it was on; if that mount disappears, captures refuse to start (`storage not available: … is not mounted`) instead of writing to the SD card. Starting also requires 200 MB free and a real write probe.
- **While recording**, every 2 s: the mount must still be there and at least 50 MB free, or the capture stops cleanly and keeps what it has.
- **FAT32:** detected from the filesystem type; the UI warns, and the writer **rolls over to `name_part2.ser`, …** before the 4 GB limit instead of failing (see [ADR 0012](0012-burst-capture-and-ram-buffer.md)).
- **Polling is read-only.** Status refresh uses `statvfs` and `access()`; the create-and-delete write probe runs only on selection and at capture start, so the UI does not write to a flash drive every few seconds.
- **Permissions:** a drive auto-mounted for a login user is not writable by a system account, so the service runs as that login user by default after a `sudo apt install` ([ADR 0010](0010-debian-package-and-updates.md)); the error message names the fix (`sudo rustflare-user <login>`). Reformatting the drive (to exFAT/ext4) is recommended in the UI but never done by Rustflare.

## Consequences

- No capture ever lands on the SD card by accident; the cost is that a removed drive blocks capturing until it is back or another location is chosen.
- Storage cannot be changed while a capture is running or flushing.
- Settings in `rustflare.json` outrank the environment: if you edit `RUSTFLARE_DATA_DIR` and nothing changes, delete the saved choice (documented).
