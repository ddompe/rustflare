# Setting up the Pi

RustFlare installs with one command, but a Pi that records for hours next to a stacker is worth ten minutes of setup. Everything below was done and checked on a Raspberry Pi 5 (8 GB, Raspberry Pi OS based on Debian 13) with a USB stick as the capture drive. When you are done, run:

```sh
rustflare-check
```

It changes nothing; it prints OK, WARN or FAIL for each item below and tells you what to do about a warning.

## 1. The capture drive

- **Use a drive, not the SD card.** Select it in More > Storage. RustFlare remembers the drive by its filesystem **UUID** and the folder on it, so it finds the drive again however the desktop names the mount (`LABEL`, then `LABEL1` after a replug). You never need to reselect it.
- **Filesystem.** FAT32 (most factory-formatted sticks) works: RustFlare splits a recording into `_part2`, `_part3` files before 4 GB. exFAT or ext4 avoids the splitting; format a drive with the same label if you want one.
- **Speed matters more than size.** The stick tested here wrote about 70 MB/s and paused for up to 1.2 s a few times a minute even with nothing else running. The RAM buffer (section 2) hides those pauses from the camera; a faster stick or an SSD makes them rarer. The Apollo-M Mini at full rate produces about 325 MB/s, far more than any stick takes, so a buffer fills during a long burst whatever you use (see *Drop frames* in the Record tab).
- **Which port: the drive on a black USB 2.0 port, the camera on a blue USB 3 port.** On the Raspberry Pi 5 a USB 3 camera and a USB 3 drive together made both drop off the bus again and again while recording, stacking and guiding ran (the camera 6 times in 3 minutes; the drive twice, which corrupted its filesystem). With the drive on a USB 2.0 port the same load ran with no drops in three runs, with the same power supply (5.1 V, no throttling). A USB 2.0 port writes about 35 MB/s: enough for bursts of 100 frames every 20 s and for the stack-only and best-frames modes, which write almost nothing. RustFlare warns in More > Storage and on the Record tab when it sees the drive on a USB 3 link next to a USB 3 camera. If the cause turns out to be power rather than the port, a powered USB hub or a 27 W supply with `usb_max_current_enable=1` are the things to try (not established). For long full-rate recordings the stick is the limit anyway (60 to 100 MB/s): see [recording to the SD card](record.md#recording-to-the-pis-own-sd-card).
- **Eject before you unplug** (More > Storage), so the stick is flushed. A pulled stick leaves a recording that can be repaired with `scripts/ser-repair.py`.
- **A fixed mount point is optional.** If you want `/mnt/astrodrive` always, add `UUID=<your uuid> /mnt/astrodrive vfat nofail,x-systemd.automount,uid=<your login>,gid=<your login> 0 0` to `/etc/fstab` (the UUID is in `rustflare-check`'s output and in `lsblk -f`) and select that folder once. Not needed otherwise.

## 2. Memory: the RAM buffer

Frames go into a RAM buffer and a separate thread writes them to the drive, so a slow drive does not slow the camera.

| Pi | Buffer (More > RAM buffer) | Why |
|---|---|---|
| 8 GB | **4096 MB** | leaves about 3.5 GB for the system, the stacker (up to about 1 GB) and the page cache. At the camera's full rate it holds about 12 seconds of video |
| 4 GB | 2048 MB | the same reasoning |

The default is 1 GB and the value is capped at 70 % of free memory. To set it without the page: `curl -sk -X PUT -H 'content-type: application/json' -d '{"mb":4096}' https://<pi>:8443/api/capture/buffer`. A bigger buffer is not better: when memory gets tight the kernel starts reclaiming and swapping, which is what stalls a recording.

**Swap.** Raspberry Pi OS swaps to compressed RAM (zram) and the default `vm.swappiness` is 60, which lets the kernel push even a recording buffer into it under pressure. Lower it:

```sh
echo 'vm.swappiness=10' | sudo tee /etc/sysctl.d/90-rustflare.conf
sudo sysctl --system
```

**Memory limits for the stacker.** Raspberry Pi OS switches the memory cgroup controller off, so the stacker's `MemoryMax` is ignored. Enable it once and reboot:

```sh
sudo rustflare-memcg      # adds cgroup_enable=memory cgroup_memory=1 to /boot/firmware/cmdline.txt (a backup is kept)
sudo reboot
```

After the reboot `rustflare-check` says the memory controller is on. If a Pi ever fails to boot after this, put the file `cmdline.txt.bak-before-memcg` back on the boot partition.

## 3. What the package already sets

You do not need to do anything for these; `rustflare-check` confirms them.

- The **recorder** runs at nice -5 with a high CPU weight and is protected from the out-of-memory killer, so it wins any contention.
- Log lines and the small state files are written by a separate low-priority thread: the thread that handles every camera frame never waits for the SD card.
- The **stacker** (always on, enabled by the package) runs at nice 19 with a CPU quota of two cores, only on cores 2 and 3 (the USB and SD-card interrupts are on core 0, and the recorder keeps 0 and 1), in the idle I/O class, and stacks only recordings the recorder has finished. Details and the measurements behind them: [architecture](../project/architecture.md#priorities-and-cpu-placement-on-the-pi) and [the telemetry runs](../project/stacker-telemetry-run-2026-11-22.md).
- If you ran the installer as a normal user, the services run as that user so they can write to the drives the desktop mounts for you (`rustflare-user` shows or changes it; the stacker follows).

## 4. Services: what to keep, what to switch off

A default Raspberry Pi OS Desktop runs a few dozen services. None of the ones below is a performance problem (the whole desktop session idles at well under 1 % CPU), but a few are worth switching off on an instrument.

**Keep (needed):** `rustflare`, `rustflare-stack`, `bluetooth` (the ZWO AM3 over Bluetooth), `avahi-daemon` (the `<name>.local` address), `NetworkManager` and `wpa_supplicant` (Wi-Fi and the hotspot), `ssh`, `systemd-timesyncd` (the Sun's position and the mount's clock depend on the time), `udisks2`, `polkit` and `accounts-daemon` (USB drives mounting and Eject), `dbus`, `systemd-journald`, `systemd-logind`, `cron`.

**Safe to switch off:**

```sh
# package work that can start in the middle of a session (update by hand between sessions instead)
sudo systemctl disable --now apt-daily.timer apt-daily-upgrade.timer
sudo systemctl mask packagekit              # the desktop's package manager
# NFS helpers: not needed unless you mount network shares, and rpcbind listens on the network
sudo systemctl disable --now nfs-blkmap rpcbind rpcbind.socket
sudo systemctl disable --now man-db.timer
```

Update the system yourself, with the Pi idle: `sudo apt update && sudo apt full-upgrade`, then reboot.

**Think twice: the desktop (`lightdm`, the compositor, the panel, the file manager).** It takes roughly 0.3 to 0.5 GB of RAM and no measurable CPU. It is also what mounts a USB drive for your login and what lets Eject work (the drive services only answer an active local session). A fully headless Pi (`sudo systemctl set-default multi-user.target`) saves that memory but needs another way to mount the drive (the `fstab` line in section 1) and has not been tested here. On an 8 GB Pi with a 4 GB buffer it is not worth the trouble.

## 5. Network, power and heat

- **Power:** use the official 27 W supply. A weak supply shows as under-voltage in `vcgencmd get_throttled` (and in `rustflare-check`) and as camera or drive disconnects.
- **Heat:** with the stacker working the Pi 5 reached 70 C in the open with no throttling; use the active cooler or a case with a fan for a day in the sun.
- **Wi-Fi power saving** can add latency to the phone's live view. To switch it off for your connection: `sudo nmcli connection modify "<connection name>" 802-11-wireless.powersave 2` (not measured here).
- **The hotspot** for use away from home is described in [Phone setup](phone-setup.md).

## 6. Checking later

`rustflare-check` any time (it takes a second). To see how the stacker behaves next to a recording, read its telemetry: [Stacking](stacking.md#telemetry-how-it-behaves-next-to-a-recording).
