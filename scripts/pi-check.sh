#!/bin/sh
# Read-only health check of a Raspberry Pi running RustFlare: is it set up the way docs/guide/pi-setup.md describes?
#   rustflare-check                (installed with the package; or scripts/pi-check.sh from a checkout; no changes are made)
# Prints OK, WARN (works, but see the guide) or FAIL for each item; the exit status is 1 if anything failed.
FAIL=0; WARN=0
ok()   { printf 'OK    %s\n' "$1"; }
warn() { printf 'WARN  %s\n' "$1"; WARN=$((WARN+1)); }
bad()  { printf 'FAIL  %s\n' "$1"; FAIL=$((FAIL+1)); }
have() { command -v "$1" >/dev/null 2>&1; }
active() { systemctl is-active --quiet "$1" 2>/dev/null; }
API=${RUSTFLARE_API:-https://localhost:8443}
status() { curl -sk -m 8 "$API/api/status" 2>/dev/null || curl -s -m 8 http://localhost:8080/api/status 2>/dev/null; }

echo "== Services"
active rustflare && ok "recorder service is running" || bad "rustflare.service is not running"
if active rustflare-stack; then ok "stacker service is running"; else warn "rustflare-stack.service is not running (stacking is off)"; fi
systemctl is-enabled --quiet rustflare 2>/dev/null && ok "recorder starts at boot" || bad "recorder is not enabled at boot"

echo "== Memory"
TOTAL=$(awk '/MemTotal/{print int($2/1024)}' /proc/meminfo)
if grep -qw memory /sys/fs/cgroup/cgroup.controllers 2>/dev/null; then ok "memory cgroup controller is on (the stacker's MemoryMax is enforced)"
else warn "memory cgroup controller is off: MemoryMax is ignored. Run: sudo rustflare-memcg, then reboot"; fi
BUF=$(status | python3 -c 'import sys,json; print(json.load(sys.stdin)["capture"]["buffer_mb"])' 2>/dev/null)
if [ -n "$BUF" ]; then
  if [ "$BUF" -gt $((TOTAL * 60 / 100)) ]; then warn "RAM buffer is ${BUF} MB of ${TOTAL} MB: more than 60 % leaves little for the system, the stacker and the page cache (4096 suits an 8 GB Pi)"
  else ok "RAM buffer is ${BUF} MB of ${TOTAL} MB"; fi
else warn "could not read the buffer size from the recorder"; fi
SW=$(cat /proc/sys/vm/swappiness); [ "$SW" -le 20 ] && ok "vm.swappiness is $SW" || warn "vm.swappiness is $SW: under memory pressure the kernel may swap the recording buffer to compressed RAM; 10 is advised"

echo "== Priorities"
P=$(pgrep -x rustflare-serve | head -1)
if [ -n "$P" ]; then
  NI=$(ps -o ni= -p "$P" | tr -d ' '); [ "$NI" -lt 0 ] && ok "recorder runs at nice $NI" || warn "recorder runs at nice $NI (the package sets -5: reinstall the package or check a unit override)"
  OOM=$(cat /proc/$P/oom_score_adj); [ "$OOM" -lt 0 ] && ok "recorder is protected from the out-of-memory killer ($OOM)" || warn "recorder's OOM score adjustment is $OOM"
fi
S=$(pgrep -x rustflare-stack | head -1)
if [ -n "$S" ]; then
  A=$(awk '/Cpus_allowed_list/{print $2}' /proc/$S/status); [ "$A" != "0-3" ] && ok "stacker is confined to cores $A" || warn "stacker may run on every core (the package pins it to 2-3 on a 4-core Pi)"
fi
GOV=$(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor 2>/dev/null); [ -n "$GOV" ] && ok "CPU governor: $GOV"

echo "== Power, heat, clock"
if have vcgencmd; then
  T=$(vcgencmd get_throttled 2>/dev/null | cut -d= -f2)
  [ "$T" = "0x0" ] && ok "no under-voltage or throttling since boot" || warn "firmware reports $T since boot (0x1/0x10000 under-voltage: use the official 27 W supply; 0x4/0x40000 throttled; 0x8/0x80000 soft temperature limit)"
  vcgencmd measure_temp 2>/dev/null | sed 's/^/OK    CPU /'
fi
[ "$(timedatectl show -p NTPSynchronized --value 2>/dev/null)" = yes ] && ok "clock is synchronised" || warn "clock is not synchronised (Go to Sun and the pointing use it); wait a minute after boot, or check the network"

echo "== Storage"
J=/var/lib/rustflare/rustflare.json
if [ -r "$J" ]; then
  DIR=$(python3 -c 'import json; print(json.load(open("'$J'")).get("data_dir",""))' 2>/dev/null)
  UUID=$(python3 -c 'import json; print(json.load(open("'$J'")).get("data_dir_uuid") or "")' 2>/dev/null)
  [ -n "$UUID" ] && ok "capture drive is remembered by its UUID ($UUID)" || warn "storage has no UUID (an internal folder, or a drive selected by an old version: select it again in More > Storage)"
  if [ -d "$DIR" ]; then
    FS=$(findmnt -no FSTYPE -T "$DIR" 2>/dev/null); FREE=$(df -Pm "$DIR" | awk 'NR==2{print $4}')
    ok "capture folder $DIR is there ($FS, ${FREE} MB free)"
    case "$FS" in vfat) warn "the drive is FAT32: files split at 4 GB (works, but exFAT or ext4 avoids it)";; esac
    case "$DIR" in /media/*|/mnt/*|/run/media/*) :;; *) warn "the capture folder is on the Pi's own SD card, not a drive";; esac
  else bad "capture folder $DIR does not exist (drive not mounted?)"; fi
else warn "no settings file yet ($J)"; fi

echo "== Services that are not needed for observing"
for u in apt-daily.timer apt-daily-upgrade.timer; do systemctl is-enabled --quiet $u 2>/dev/null && warn "$u is enabled: package work can start in the middle of a session (guide: Services)"; done
active packagekit && warn "PackageKit is running (package manager of the desktop; not needed)"
active nfs-blkmap && warn "nfs-blkmap is running and not needed unless you use NFS"
active rpcbind && warn "rpcbind is listening on the network and is not needed unless you use NFS"
active bluetooth && ok "bluetooth is on (needed for the ZWO AM3 over Bluetooth)" || warn "bluetooth is off (needed only for the AM3 over Bluetooth)"
active avahi-daemon && ok "avahi is on (the <name>.local address works)" || warn "avahi is off: <name>.local will not resolve, use the IP address"

echo
echo "$FAIL failed, $WARN warnings"
[ "$FAIL" = 0 ]
