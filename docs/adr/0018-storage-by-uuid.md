# 0018. Remember the capture drive by its UUID

- Status: Accepted
- Date: 2026-11-22

## Context

The saved storage choice was a path such as `/media/astropi/ASTRODRIVE/rustflare`. The desktop auto-mounter names a drive's mount point after its label and appends a digit when a stale mount point of that name is left over, so the same stick turned up as `.../ASTRODRIVE1` after a replug and RustFlare reported its storage as missing (and the stacker, which follows the recorder's folder, lost its files).

## Decision

When a folder is selected, remember the drive's filesystem **UUID** (`/dev/disk/by-uuid`) and the folder relative to the mount point (`data_dir_uuid`, `data_dir_rel` in the settings file). Every use of the storage folder resolves the UUID to the drive's *current* mount point (checked at most every 2 seconds). Settings saved by older versions are upgraded on start: if the drive is mounted its UUID is learned, and if the old mount point is gone and exactly one mount in the same folder is named the old name followed by digits, that mount is adopted. Nothing is guessed when two mounts fit.

Not done: forcing a fixed mount point (an fstab line or a systemd mount unit). It needs root, differs per desktop and fixes only one symptom; following the UUID makes the name irrelevant. A user who wants a fixed name can still add `UUID=... /mnt/astrodrive vfat nofail,x-systemd.automount 0 0` to `/etc/fstab` and select that folder.

## Consequences

- A replug under another mount name no longer interrupts anything between recordings; a recording that is running when the drive is pulled still stops, as before.
- The stacker and the web page follow automatically (they read the recorder's `capture.state`).
- A drive without a UUID (some exotic filesystems) keeps the path behaviour.
