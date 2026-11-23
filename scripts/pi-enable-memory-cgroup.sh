#!/bin/sh
# Raspberry Pi OS turns the memory cgroup controller off (the firmware adds cgroup_disable=memory), so systemd's MemoryMax= on the stacker
# is ignored. This adds cgroup_enable=memory cgroup_memory=1 to /boot/firmware/cmdline.txt (a backup is kept) and says to reboot.
#   sudo rustflare-memcg            (installed with the package; or scripts/pi-enable-memory-cgroup.sh from a checkout)
set -eu
F=/boot/firmware/cmdline.txt
[ "$(id -u)" = 0 ] || { echo "run as root: sudo $0" >&2; exit 1; }
[ -f "$F" ] || { echo "$F not found: this is not a Raspberry Pi OS boot partition" >&2; exit 1; }
if grep -q "cgroup_memory=1" "$F"; then echo "already set in $F"; exit 0; fi
[ "$(wc -l < "$F")" -le 1 ] || { echo "$F has more than one line; edit it by hand" >&2; exit 1; }
cp -p "$F" "$F.bak-before-memcg"
printf '%s cgroup_enable=memory cgroup_memory=1' "$(tr -d '\n' < "$F")" > "$F"
echo "added to $F (backup: $F.bak-before-memcg); reboot to apply: sudo reboot"
echo "afterwards: cat /sys/fs/cgroup/cgroup.controllers  should list memory"
