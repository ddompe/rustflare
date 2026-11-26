//! Where captures are stored: selectable at run time (e.g. a USB drive), persisted, and guarded
//! against the drive being removed, full, or on a filesystem with a file-size limit (ADR 0011).

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Refuse to start a capture with less free space than this.
pub const MIN_FREE_BYTES: u64 = 200 * 1024 * 1024;
/// Stop a running capture when free space drops below this.
pub const STOP_FREE_BYTES: u64 = 50 * 1024 * 1024;
/// The default reserve kept free on the internal disk.
const DEFAULT_RESERVE_GB: f64 = 8.0;
/// Extra room the internal disk's guard leaves over its reserve and the frames in memory.
const INTERNAL_MARGIN: u64 = 1_000_000_000;
const FAT_MAX_FILE: u64 = 4 * 1024 * 1024 * 1024 - 1;

#[derive(Clone, Debug, PartialEq)]
pub struct Mount {
    pub device: String,
    pub point: PathBuf,
    pub fstype: String,
    /// Mounted read-only (the kernel does this when a filesystem has errors, for example after the drive was pulled without ejecting).
    pub read_only: bool,
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
/// Parses /proc/mounts text (spaces in paths are escaped as `\040`).
pub fn parse_mounts(text: &str) -> Vec<Mount> {
    fn unescape(s: &str) -> String {
        let mut out = String::new();
        let b: Vec<char> = s.chars().collect();
        let mut i = 0;
        while i < b.len() {
            if b[i] == '\\' && i + 3 < b.len() + 0 && b[i + 1..i + 4].iter().all(|c| c.is_digit(8)) {
                let n = u32::from_str_radix(&b[i + 1..i + 4].iter().collect::<String>(), 8).unwrap_or(63);
                out.push(char::from_u32(n).unwrap_or('?'));
                i += 4;
            } else {
                out.push(b[i]);
                i += 1;
            }
        }
        out
    }
    text.lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            let (device, point, fstype) = (unescape(f.next()?), PathBuf::from(unescape(f.next()?)), f.next()?.to_string());
            let read_only = f.next().is_some_and(|o| o.split(',').any(|x| x == "ro"));
            Some(Mount { device, point, fstype, read_only })
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn read_mounts() -> Vec<Mount> {
    std::fs::read_to_string("/proc/mounts").map(|t| parse_mounts(&t)).unwrap_or_default()
}

/// Development fallback (macOS): treat entries of /Volumes as mounts.
#[cfg(not(target_os = "linux"))]
fn read_mounts() -> Vec<Mount> {
    std::fs::read_dir("/Volumes")
        .map(|d| d.flatten().map(|e| Mount { device: String::new(), point: e.path(), fstype: "unknown".into(), read_only: false }).collect())
        .unwrap_or_default()
}

/// The most specific mount containing `path`, ignoring the root filesystem.
pub fn containing_mount<'a>(path: &Path, mounts: &'a [Mount]) -> Option<&'a Mount> {
    mounts
        .iter()
        .filter(|m| m.point != Path::new("/") && path.starts_with(&m.point))
        .max_by_key(|m| m.point.as_os_str().len())
}

pub fn max_file_bytes(fstype: &str) -> Option<u64> {
    matches!(fstype, "vfat" | "msdos" | "fat" | "fat32").then_some(FAT_MAX_FILE)
}

fn space(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return None;
    }
    let fr = s.f_frsize as u64;
    Some((s.f_blocks as u64 * fr, s.f_bavail as u64 * fr))
}

/// Cheap check (no I/O on the drive) used for polling: does the OS permit writing here?
fn may_write(dir: &Path) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(dir.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    if unsafe { libc::access(c.as_ptr(), libc::W_OK | libc::X_OK) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}

/// Real check: create and delete a file. Only used when selecting a location or starting a capture.
fn writable(dir: &Path) -> Result<(), String> {
    let probe = dir.join(format!(".rustflare-write-test-{}", std::process::id()));
    std::fs::write(&probe, b"x").map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

#[derive(Clone, Debug, Serialize)]
pub struct Info {
    pub path: String,
    pub label: String,
    pub mount_point: Option<String>,
    pub fstype: Option<String>,
    pub total: Option<u64>,
    pub free: Option<u64>,
    /// Largest file this filesystem can hold (FAT32: 4 GiB − 1).
    pub max_file_bytes: Option<u64>,
    /// Usable now: exists, mounted (if it was on a removable mount), writable.
    pub available: bool,
    pub problem: Option<String>,
    /// The Pi's own disk (the SD card), where recording is allowed but a reserve is always kept free for the system.
    pub internal: bool,
    /// Bytes kept free on an internal disk (0 for a removable drive).
    pub reserve_bytes: u64,
    /// Something the user should do about how the drive is connected (a USB 3 drive next to a USB 3 camera), in words.
    pub warning: Option<String>,
}

struct State {
    dir: PathBuf,
    /// Mount point the directory was on when selected; it must still be mounted to use the directory.
    required_mount: Option<PathBuf>,
    /// The drive's filesystem UUID and the folder inside it. The mount point's name is not stable (the desktop mounts a drive as
    /// `/media/<user>/<LABEL>` and as `<LABEL>1` when a stale mount point of that name is left over), the UUID is.
    uuid: Option<String>,
    rel: Option<PathBuf>,
    checked: Option<std::time::Instant>,
}

/// Filesystem UUID of a block device, from `/dev/disk/by-uuid`.
#[cfg(target_os = "linux")]
fn uuid_of(device: &str) -> Option<String> {
    let dev = std::fs::canonicalize(device).ok()?;
    std::fs::read_dir("/dev/disk/by-uuid").ok()?.flatten().find(|e| std::fs::canonicalize(e.path()).ok().as_deref() == Some(dev.as_path())).map(|e| e.file_name().to_string_lossy().to_string())
}

#[cfg(not(target_os = "linux"))]
fn uuid_of(_device: &str) -> Option<String> { None }

/// Where the drive with `uuid` is mounted now, and the stored folder on it: (directory, mount point).
pub fn resolve_uuid(uuid: &str, rel: &Path, mounts: &[Mount], uuid_of: &dyn Fn(&str) -> Option<String>) -> Option<(PathBuf, PathBuf)> {
    mounts.iter().filter(|m| m.device.starts_with('/')).find(|m| uuid_of(&m.device).as_deref() == Some(uuid)).map(|m| (m.point.join(rel), m.point.clone()))
}

/// The mount point a path on a removable drive was under: `/media/<user>/<LABEL>`, `/run/media/<user>/<LABEL>`, `/media/<LABEL>`, `/mnt/<name>`.
fn removable_root(path: &Path) -> Option<PathBuf> {
    let parts: Vec<_> = path.components().collect();
    let n = match parts.get(1).and_then(|c| c.as_os_str().to_str())? {
        "media" => if parts.len() >= 4 { 4 } else { 3 }, // /media/<user>/<LABEL> (a bare /media/<LABEL> has no user part)
        "run" if parts.get(2).and_then(|c| c.as_os_str().to_str()) == Some("media") => 5,
        "mnt" => 3,
        _ => return None,
    };
    (parts.len() >= n).then(|| parts[..n].iter().collect())
}

/// A drive that used to be at `old_root` and is mounted under a slightly different name now (`ASTRODRIVE` became `ASTRODRIVE1`):
/// the one mount in the same folder whose name is the old name followed by digits. Only an unambiguous match counts.
pub fn renamed_mount(old_root: &Path, mounts: &[Mount]) -> Option<Mount> {
    let (parent, name) = (old_root.parent()?, old_root.file_name()?.to_string_lossy().to_string());
    let hits: Vec<&Mount> = mounts.iter().filter(|m| m.point.parent() == Some(parent) && m.point.file_name().map_or(false, |n| { let n = n.to_string_lossy(); n.starts_with(&name) && n.len() > name.len() && n[name.len()..].chars().all(|c| c.is_ascii_digit()) })).collect();
    (hits.len() == 1).then(|| hits[0].clone())
}

pub struct Storage {
    state: Mutex<State>,
    config_file: PathBuf,
    /// The Pi's own data folder (the default): recording here is allowed, with a reserve kept free.
    internal_dir: PathBuf,
}

impl Storage {
    /// Uses the saved choice in `config_file` if there is one, otherwise `default_dir`.
    pub fn load(config_file: PathBuf, default_dir: PathBuf) -> Storage {
        let internal_dir = default_dir.clone();
        let saved = crate::config::read(&config_file).get("data_dir").and_then(|v| v.as_str()).map(PathBuf::from);
        let (dir, from_saved) = match saved {
            Some(d) => (d, true),
            None => (default_dir, false),
        };
        if from_saved {
            log::info!("storage: using saved choice {} (from {}; overrides --data-dir)", dir.display(), config_file.display());
        }
        let cfg = crate::config::read(&config_file);
        let mut uuid = cfg.get("data_dir_uuid").and_then(|v| v.as_str()).map(String::from);
        let mut rel = cfg.get("data_dir_rel").and_then(|v| v.as_str()).map(PathBuf::from);
        let mounts = read_mounts();
        let mut dir = dir;
        let mut required_mount = containing_mount(&dir, &mounts).map(|m| m.point.clone());
        if uuid.is_none() && from_saved {
            // an older settings file: learn the drive's UUID now if it is mounted, or find it under its new name if the name changed
            if let Some(m) = containing_mount(&dir, &mounts).cloned().filter(|m| m.device.starts_with('/')) {
                (uuid, rel) = (uuid_of(&m.device), dir.strip_prefix(&m.point).ok().map(|r| r.to_path_buf()));
            } else if let Some(old_root) = removable_root(&dir) {
                if let Some(m) = renamed_mount(&old_root, &mounts) {
                    if let (Some(u), Ok(r)) = (uuid_of(&m.device), dir.strip_prefix(&old_root)) {
                        log::info!("storage: {} is now mounted as {}; following it", old_root.display(), m.point.display());
                        (uuid, rel) = (Some(u), Some(r.to_path_buf()));
                        dir = m.point.join(r);
                        required_mount = Some(m.point);
                    }
                }
            }
            if let (Some(u), Some(r)) = (&uuid, &rel) {
                let _ = crate::config::set(&config_file, "data_dir", serde_json::json!(dir));
                let _ = crate::config::set(&config_file, "data_dir_uuid", serde_json::json!(u));
                let _ = crate::config::set(&config_file, "data_dir_rel", serde_json::json!(r));
            }
        }
        if !dir.exists() && required_mount.is_none() {
            let _ = std::fs::create_dir_all(&dir);
        }
        Storage { state: Mutex::new(State { dir, required_mount, uuid, rel, checked: None }), config_file, internal_dir }
    }

    pub fn config_file(&self) -> &Path {
        &self.config_file
    }

    /// The always-on logs (events, mount commands) live next to the settings file, on the Pi's own disk, so they survive the
    /// capture drive being ejected. They are kept inside a size and age limit (see `logs::prune`).
    pub fn internal_log_dir(&self) -> PathBuf {
        self.config_file.parent().map(|p| p.join("logs")).unwrap_or_else(|| PathBuf::from("logs"))
    }

    /// Follows the drive to wherever it is mounted now (checked at most every 2 s). A drive that is not mounted leaves the last
    /// path in place, which then reports as unavailable.
    fn follow_drive(&self) {
        let mut s = self.state.lock().unwrap();
        let (Some(uuid), Some(rel)) = (s.uuid.clone(), s.rel.clone()) else { return };
        if s.checked.map_or(false, |t| t.elapsed() < std::time::Duration::from_secs(2)) {
            return;
        }
        s.checked = Some(std::time::Instant::now());
        if let Some((dir, point)) = resolve_uuid(&uuid, &rel, &read_mounts(), &uuid_of) {
            if dir != s.dir {
                log::info!("storage: the drive is mounted at {} now (was {})", point.display(), s.dir.display());
                s.dir = dir;
            }
            s.required_mount = Some(point);
            let dir = s.dir.clone();
            drop(s);
            let _ = crate::config::set(&self.config_file, "data_dir", serde_json::json!(dir));
        }
    }

    pub fn dir(&self) -> PathBuf {
        self.follow_drive();
        self.state.lock().unwrap().dir.clone()
    }

    pub fn info(&self) -> Info {
        self.follow_drive();
        let s = self.state.lock().unwrap();
        let mut i = describe(&s.dir, s.required_mount.as_deref(), &read_mounts());
        self.mark_internal(&mut i, &s.dir);
        i
    }

    /// Marks `i` as the Pi's own disk and gives it its reserve (the label says what it is, whatever the folder is called).
    fn mark_internal(&self, i: &mut Info, dir: &Path) {
        if dir == self.internal_dir && i.mount_point.is_none() {
            i.internal = true;
            i.reserve_bytes = self.reserve_bytes();
            i.label = "Internal storage (SD card)".into();
        }
    }

    /// How much of the internal disk is always kept free (the system, its logs and the settings live there): 2 to 100 GB, 8 by default.
    pub fn reserve_bytes(&self) -> u64 {
        let gb = crate::config::read(&self.config_file).get("internal_reserve_gb").and_then(|v| v.as_f64()).unwrap_or(DEFAULT_RESERVE_GB);
        (gb.clamp(2.0, 100.0) * 1e9) as u64
    }

    pub fn set_reserve_gb(&self, gb: f64) -> Result<f64, String> {
        if !(2.0..=100.0).contains(&gb) {
            return Err("keep between 2 and 100 GB free".into());
        }
        crate::config::set(&self.config_file, "internal_reserve_gb", serde_json::json!(gb))?;
        Ok(gb)
    }

    /// Fails with a user-facing message unless captures can safely be written now.
    pub fn ensure_ready(&self) -> Result<PathBuf, String> {
        let i = self.info();
        if !i.available {
            return Err(format!("storage not available: {}", i.problem.unwrap_or_default()));
        }
        if i.internal {
            if let Some(f) = i.free {
                if f < i.reserve_bytes + MIN_FREE_BYTES {
                    return Err(format!("the internal disk has {:.1} GB free and RustFlare keeps {:.0} GB of it free for the system: free some space, or lower the reserve in More > Storage", f as f64 / 1e9, i.reserve_bytes as f64 / 1e9));
                }
            }
        }
        if i.free.is_some_and(|f| f < MIN_FREE_BYTES) {
            return Err(format!("storage almost full ({} MB free)", i.free.unwrap_or(0) / 1_000_000));
        }
        let dir = self.dir();
        writable(&dir).map_err(|e| format!("storage not writable: {e}{}", perm_hint_str(&e)))?;
        Ok(dir)
    }

    /// Cheap check used while recording.
    /// `pending` is what is still in memory waiting to be written: it will use space too.
    pub fn running_problem(&self, pending: u64) -> Option<String> {
        let i = self.info();
        if !i.available {
            return Some(format!("storage not available: {}", i.problem.unwrap_or_default()));
        }
        guard(i.free, i.internal, i.reserve_bytes, pending)
    }

    /// Drives and other mounted volumes that could hold captures (target folder: `<mount>/rustflare`).
    pub fn candidates(&self) -> Vec<Info> {
        let mounts = read_mounts();
        let mut internal = describe(&self.internal_dir, None, &mounts);
        self.mark_internal(&mut internal, &self.internal_dir);
        let mut out = vec![internal];
        out.extend(mounts.iter().filter(|m| ["/media", "/mnt", "/run/media", "/Volumes"].iter().any(|p| m.point.starts_with(p))).map(|m| {
            // judged on the drive itself: the `rustflare` folder is created when selected
            let mut i = describe(&m.point, Some(&m.point), &mounts);
            i.path = m.point.join("rustflare").display().to_string();
            i
        }));
        out
    }

    /// Safely removes a drive: flushes it, unmounts it and (where possible) powers the port down. `mount_point` must
    /// be one of `candidates()`. Returns a message for the user.
    pub fn eject(&self, mount_point: &Path) -> Result<String, String> {
        let mounts = read_mounts();
        let m = mounts
            .iter()
            .find(|m| m.point == mount_point && self.candidates().iter().any(|c| Path::new(&c.path).parent() == Some(&m.point)))
            .ok_or("that is not a removable drive")?
            .clone();
        // flush this filesystem only
        if let Ok(f) = std::fs::File::open(&m.point) {
            #[cfg(target_os = "linux")]
            {
                use std::os::fd::AsRawFd;
                unsafe { libc::syncfs(f.as_raw_fd()) };
            }
            #[cfg(not(target_os = "linux"))]
            let _ = f;
        }
        let run = |prog: &str, args: &[&str]| -> Result<(), String> {
            let o = std::process::Command::new(prog).args(args).output().map_err(|e| format!("{prog}: {e}"))?;
            if o.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&o.stderr).trim().to_string()) }
        };
        run("udisksctl", &["unmount", "-b", &m.device])
            .or_else(|_| run("sudo", &["-n", "umount", &m.point.display().to_string()]))
            .map_err(|e| {
                let busy = if e.contains("busy") { " Something still has the drive open: a terminal whose current folder is on the drive (type `cd ~` there), a file viewer, or another program. RustFlare closes its own logs before ejecting." } else { "" };
                format!("could not unmount: {e}.{busy}")
            })?;
        // best effort: power off the whole device so it can be pulled safely
        let parent = std::process::Command::new("lsblk").args(["-no", "pkname", &m.device]).output().ok().and_then(|o| String::from_utf8(o.stdout).ok()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        let powered = parent.as_deref().map(|p| run("udisksctl", &["power-off", "-b", &format!("/dev/{p}")]).is_ok()).unwrap_or(false);
        log::info!("storage: ejected {} ({})", m.point.display(), if powered { "powered off" } else { "unmounted" });
        Ok(if powered { "Safe to remove the drive.".into() } else { "Unmounted: safe to remove the drive.".into() })
    }

    /// Selects a directory (created if missing) after verifying it is writable, and saves the choice.
    pub fn select(&self, path: &Path) -> Result<Info, String> {
        if !path.is_absolute() {
            return Err("path must be absolute".into());
        }
        let mounts = read_mounts();
        if let Some(m) = containing_mount(path, &mounts) {
            if !path.starts_with(&m.point) || !m.point.exists() {
                return Err("mount point is not available".into());
            }
        }
        std::fs::create_dir_all(path).map_err(|e| format!("cannot create {}: {e}{}", path.display(), perm_hint(&e)))?;
        writable(path).map_err(|e| format!("{} is not writable by the Rustflare service: {e}{}", path.display(), perm_hint_str(&e)))?;
        let dir = path.canonicalize().map_err(|e| e.to_string())?;
        let mount = containing_mount(&dir, &mounts).cloned();
        let required_mount = mount.as_ref().map(|m| m.point.clone());
        let (uuid, rel) = match mount.as_ref().filter(|m| m.device.starts_with('/')) {
            Some(m) => (uuid_of(&m.device), dir.strip_prefix(&m.point).ok().map(|r| r.to_path_buf())),
            None => (None, None),
        };
        crate::config::set(&self.config_file, "data_dir", serde_json::json!(dir))?;
        // the drive is remembered by its UUID, so a different mount name next time does not matter
        crate::config::set(&self.config_file, "data_dir_uuid", serde_json::json!(uuid))?;
        crate::config::set(&self.config_file, "data_dir_rel", serde_json::json!(rel))?;
        let mut s = self.state.lock().unwrap();
        s.dir = dir;
        s.required_mount = required_mount;
        s.uuid = uuid;
        s.rel = rel;
        s.checked = Some(std::time::Instant::now());
        let mut i = describe(&s.dir, s.required_mount.as_deref(), &mounts);
        self.mark_internal(&mut i, &s.dir);
        Ok(i)
    }
}

/// Should a running recording stop for lack of space? On a removable drive when under 50 MB would be left; on the internal disk when
/// what is left after the frames still in memory are written would eat into the reserve kept for the system.
pub fn guard(free: Option<u64>, internal: bool, reserve: u64, pending: u64) -> Option<String> {
    let f = free?;
    if internal {
        // a margin of 1 GB: the check runs every 2 s and the disk takes a few hundred MB meanwhile
        let need = reserve + pending.min(f) + INTERNAL_MARGIN;
        (f < need + STOP_FREE_BYTES).then(|| format!("the internal disk is down to {:.1} GB free (keeping {:.0} GB for the system, and {:.1} GB still has to be written from memory)", f as f64 / 1e9, reserve as f64 / 1e9, pending as f64 / 1e9))
    } else {
        (f < STOP_FREE_BYTES).then(|| format!("disk full ({} MB left)", f / 1_000_000))
    }
}

fn perm_hint(e: &std::io::Error) -> &'static str {
    if e.kind() == std::io::ErrorKind::PermissionDenied { PERM_HINT } else { "" }
}
fn perm_hint_str(e: &str) -> &'static str {
    if e.to_lowercase().contains("permission denied") { PERM_HINT } else { "" }
}
const PERM_HINT: &str = " (permission denied: auto-mounted drives belong to your login user; run `sudo rustflare-user <your-login>` to run the service as that user)";

/// The warning for a drive on a SuperSpeed (5 Gbps) USB link while other SuperSpeed devices (a USB 3 camera) share the machine. On the Pi 5,
/// 2026-11-26: with the drive on a USB 3 port the camera and the drive both dropped off the bus again and again under load (recording,
/// stacking, guiding); on a USB 2.0 port the same load gave no drops in three runs.
pub fn usb3_warning(drive_mbps: u32, other_superspeed: &[String]) -> Option<String> {
    (drive_mbps >= 5000 && !other_superspeed.is_empty()).then(|| {
        format!("This drive is on a USB 3 port, like {}. On the Raspberry Pi 5 that made the camera and the drive drop off the USB bus under load (recording, stacking and guiding together), which can corrupt the drive. Move the drive to a black USB 2.0 port (it writes about 35 MB/s there, enough for bursts and for the stack-only modes), then wait for it to mount.", other_superspeed.join(", "))
    })
}

/// Reads the USB link of the device behind `/dev/sdX1` and of the other devices from sysfs (Linux only).
#[cfg(target_os = "linux")]
fn usb_warning(device: &str) -> Option<String> {
    let name = device.strip_prefix("/dev/")?;
    let real = std::fs::canonicalize(format!("/sys/class/block/{name}")).ok()?;
    if !real.to_string_lossy().contains("/usb") {
        return None;
    }
    let read = |dir: &Path, f: &str| std::fs::read_to_string(dir.join(f)).ok().map(|s| s.trim().to_string());
    // the USB device is the nearest parent that has an idVendor
    let usb = real.ancestors().find(|a| a.join("idVendor").exists())?.to_path_buf();
    let speed: u32 = read(&usb, "speed")?.parse().ok()?;
    let mut others = vec![];
    for e in std::fs::read_dir("/sys/bus/usb/devices").ok()?.flatten() {
        let n = e.file_name().to_string_lossy().to_string();
        if n.contains(':') || n.starts_with("usb") {
            continue; // interfaces and root hubs
        }
        let p = std::fs::canonicalize(e.path()).ok()?;
        if p == usb || read(&p, "bDeviceClass").as_deref() == Some("09") {
            continue; // the drive itself, hubs
        }
        if read(&p, "speed").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0) >= 5000 {
            others.push(read(&p, "product").unwrap_or_else(|| "another USB 3 device".into()));
        }
    }
    usb3_warning(speed, &others)
}
#[cfg(not(target_os = "linux"))]
fn usb_warning(_device: &str) -> Option<String> {
    None
}

/// What is wrong with a mounted drive that is still listed: unplugged without ejecting (its device is gone but the mount is left over),
/// or mounted read-only because the kernel found errors in the filesystem (what a pulled drive comes back as). Said in words, with what to do.
fn drive_trouble(m: &Mount) -> Option<String> {
    let gone = m.device.starts_with("/dev/") && !Path::new(&m.device).exists();
    if gone {
        return Some(format!("{} was unplugged without being ejected: its mount is left over and cannot be written. Plug the drive in again; if it comes back under a new name the app finds it by its label, otherwise choose it in More > Storage. Eject before unplugging next time.", m.point.display()));
    }
    if m.read_only && !matches!(m.fstype.as_str(), "iso9660" | "squashfs" | "erofs") {
        return Some(format!("{} is mounted read-only: the filesystem has errors, usually after the drive was pulled without ejecting. Nothing can be recorded to it until it is checked: unmount it and run `sudo fsck.vfat -a {}` (or `fsck -fy`) on the Pi, then plug it in again.", m.point.display(), if m.device.is_empty() { "<device>" } else { &m.device }));
    }
    None
}

fn describe(dir: &Path, required_mount: Option<&Path>, mounts: &[Mount]) -> Info {
    let label = required_mount
        .and_then(|m| m.file_name())
        .or_else(|| dir.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| dir.display().to_string());
    let mount = containing_mount(dir, mounts);
    let mut problem = None;
    if let Some(req) = required_mount {
        if !mounts.iter().any(|m| m.point == req) {
            problem = Some(format!("{} is not mounted (drive removed?)", req.display()));
        }
    }
    if problem.is_none() {
        problem = mount.and_then(drive_trouble);
    }
    let exists = dir.is_dir();
    if problem.is_none() && !exists {
        // For a candidate the folder is simply created on selection; only flag a missing *current* dir.
        problem = Some(format!("{} does not exist", dir.display()));
    }
    let probe = if exists { dir } else { required_mount.unwrap_or(dir) };
    let (total, free) = if problem.as_deref().is_some_and(|p| p.contains("not mounted")) { (None, None) } else { space(probe).unzip() };
    if problem.is_none() {
        if let Err(e) = may_write(dir) {
            problem = Some(format!("not writable: {e}{}", perm_hint_str(&e)));
        }
    }
    let fstype = mount.map(|m| m.fstype.clone());
    Info {
        path: dir.display().to_string(),
        label,
        mount_point: mount.map(|m| m.point.display().to_string()),
        max_file_bytes: fstype.as_deref().and_then(max_file_bytes),
        fstype,
        total,
        free,
        available: problem.is_none(),
        problem,
        internal: false,
        reserve_bytes: 0,
        warning: mount.and_then(|m| usb_warning(&m.device)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mounts_with_escaped_spaces() {
        let m = parse_mounts("/dev/sda1 /media/user/MY\\040DRIVE vfat rw,nosuid 0 0\nproc /proc proc rw 0 0\n");
        assert_eq!(m[0].point, PathBuf::from("/media/user/MY DRIVE"));
        assert_eq!(m[0].fstype, "vfat");
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn picks_most_specific_mount_and_ignores_root() {
        let m = parse_mounts("/dev/a / ext4 rw 0 0\n/dev/b /media/x vfat rw 0 0\n/dev/c /media/x/inner ext4 rw 0 0\n");
        assert_eq!(containing_mount(Path::new("/media/x/inner/d"), &m).unwrap().fstype, "ext4");
        assert_eq!(containing_mount(Path::new("/media/x/d"), &m).unwrap().fstype, "vfat");
        assert!(containing_mount(Path::new("/var/lib"), &m).is_none());
    }

    #[test]
    fn fat_has_a_file_limit_others_do_not() {
        assert_eq!(max_file_bytes("vfat"), Some(4 * 1024 * 1024 * 1024 - 1));
        assert_eq!(max_file_bytes("exfat"), None);
        assert_eq!(max_file_bytes("ext4"), None);
    }

    #[test]
    fn select_persists_and_reloads() {
        let base = std::env::temp_dir().join(format!("rustflare-storage-test-{}", std::process::id()));
        let cfg = base.join("cfg.json");
        std::fs::create_dir_all(&base).unwrap();
        let st = Storage::load(cfg.clone(), base.join("default"));
        assert_eq!(st.dir(), base.join("default"));
        let info = st.select(&base.join("chosen")).unwrap();
        assert!(info.available && info.free.is_some(), "{info:?}");
        let again = Storage::load(cfg, base.join("default"));
        assert_eq!(again.dir(), base.join("chosen").canonicalize().unwrap());
        assert!(again.ensure_ready().is_ok());
        assert!(st.select(Path::new("relative/path")).is_err());
        std::fs::remove_dir_all(&base).ok();
    }

    fn mount(dev: &str, point: &str) -> Mount { Mount { device: dev.into(), point: point.into(), fstype: "vfat".into(), read_only: false } }

    #[test]
    fn a_drive_is_found_again_by_its_uuid_whatever_its_mount_name() {
        let uuids = |d: &str| match d { "/dev/sda1" => Some("ABCD-1234".to_string()), "/dev/sdb1" => Some("FFFF-0000".to_string()), _ => None };
        let rel = Path::new("rustflare");
        // mounted as ASTRODRIVE
        let m1 = [mount("/dev/mmcblk0p2", "/"), mount("/dev/sda1", "/media/astropi/ASTRODRIVE")];
        assert_eq!(resolve_uuid("ABCD-1234", rel, &m1, &uuids), Some((PathBuf::from("/media/astropi/ASTRODRIVE/rustflare"), PathBuf::from("/media/astropi/ASTRODRIVE"))));
        // the same drive, now mounted as ASTRODRIVE1, and another drive that must not be confused with it
        let m2 = [mount("/dev/sdb1", "/media/astropi/ASTRODRIVE"), mount("/dev/sda1", "/media/astropi/ASTRODRIVE1")];
        assert_eq!(resolve_uuid("ABCD-1234", rel, &m2, &uuids).unwrap().0, PathBuf::from("/media/astropi/ASTRODRIVE1/rustflare"));
        // not mounted at all
        assert_eq!(resolve_uuid("ABCD-1234", rel, &[mount("/dev/mmcblk0p2", "/")], &uuids), None);
    }

    #[test]
    fn an_old_setting_follows_a_renamed_mount_only_when_it_is_unambiguous() {
        let old = removable_root(Path::new("/media/astropi/ASTRODRIVE/rustflare")).unwrap();
        assert_eq!(old, PathBuf::from("/media/astropi/ASTRODRIVE"));
        assert_eq!(removable_root(Path::new("/run/media/pi/DISK/x/y")).unwrap(), PathBuf::from("/run/media/pi/DISK"));
        assert_eq!(removable_root(Path::new("/var/lib/rustflare/captures")), None);
        let one = [mount("/dev/sda1", "/media/astropi/ASTRODRIVE1"), mount("/dev/sdc1", "/media/astropi/OTHER")];
        assert_eq!(renamed_mount(&old, &one).unwrap().point, PathBuf::from("/media/astropi/ASTRODRIVE1"));
        let two = [mount("/dev/sda1", "/media/astropi/ASTRODRIVE1"), mount("/dev/sdb1", "/media/astropi/ASTRODRIVE2")];
        assert!(renamed_mount(&old, &two).is_none(), "two candidates: do not guess");
        assert!(renamed_mount(&old, &[mount("/dev/sda1", "/media/astropi/ASTRODRIVEX")]).is_none(), "only digits may follow the old name");
    }

    #[test]
    fn a_usb3_drive_next_to_a_usb3_camera_is_warned_about_and_a_usb2_drive_is_not() {
        let cam = vec!["Apollo-M MINI".to_string()];
        let w = usb3_warning(5000, &cam).unwrap();
        assert!(w.contains("USB 2.0") && w.contains("Apollo-M MINI"), "{w}");
        assert_eq!(usb3_warning(480, &cam), None, "on a USB 2.0 port there is nothing to say");
        assert_eq!(usb3_warning(5000, &[]), None, "no other SuperSpeed device: no conflict");
    }

    #[test]
    fn a_left_over_or_read_only_mount_is_explained() {
        let m = parse_mounts("/dev/sdb1 /media/x/DRIVE vfat rw,nosuid 0 0\n/dev/sdc1 /media/x/OTHER vfat ro,nosuid 0 0\n");
        assert!(!m[0].read_only && m[1].read_only);
        let stale = Mount { device: "/dev/does-not-exist-sdz9".into(), point: "/media/x/GONE".into(), fstype: "vfat".into(), read_only: false };
        assert!(drive_trouble(&stale).unwrap().contains("unplugged without being ejected"));
        let ro = Mount { device: "/dev/null".into(), point: "/media/x/DIRTY".into(), fstype: "vfat".into(), read_only: true };
        assert!(drive_trouble(&ro).unwrap().contains("fsck.vfat"));
        let fine = Mount { device: "/dev/null".into(), point: "/media/x/OK".into(), fstype: "vfat".into(), read_only: false };
        assert_eq!(drive_trouble(&fine), None);
    }

    #[test]
    fn the_internal_disk_keeps_a_reserve_for_the_system_and_counts_what_is_still_in_memory() {
        const GB: u64 = 1_000_000_000;
        // a removable drive stops at 50 MB
        assert_eq!(guard(Some(10 * GB), false, 0, 4 * GB), None);
        assert!(guard(Some(40_000_000), false, 0, 0).is_some());
        // the internal disk: 8 GB kept, plus what is queued
        assert_eq!(guard(Some(20 * GB), true, 8 * GB, 4 * GB), None);
        assert!(guard(Some(11 * GB), true, 8 * GB, 4 * GB).is_some(), "11 GB free, 4 GB still to write: the reserve would be eaten");
        assert!(guard(Some(12_500_000_000), true, 8 * GB, 4 * GB).is_some(), "12.5 GB free is inside the 1 GB margin");
        assert_eq!(guard(Some(13_200_000_000), true, 8 * GB, 4 * GB), None);
        assert!(guard(Some(8 * GB), true, 8 * GB, 0).is_some());
        assert_eq!(guard(None, true, 8 * GB, 0), None);
    }

    #[test]
    fn the_internal_disk_is_offered_as_a_place_to_record_with_its_reserve() {
        let d = std::env::temp_dir().join(format!("rf-internal-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let s = Storage::load(d.join("cfg.json"), d.join("captures"));
        let c = s.candidates();
        let i = c.iter().find(|i| i.internal).expect("the internal disk is a candidate");
        assert_eq!(i.reserve_bytes, 8_000_000_000);
        assert!(s.info().internal, "the default folder is the internal disk");
        s.set_reserve_gb(12.0).unwrap();
        assert_eq!(s.reserve_bytes(), 12_000_000_000);
        assert!(s.set_reserve_gb(1.0).is_err() && s.set_reserve_gb(500.0).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn removed_drive_is_unavailable() {
        let mounts = parse_mounts("/dev/a / ext4 rw 0 0\n");
        let i = describe(Path::new("/media/u/DRIVE/rustflare"), Some(Path::new("/media/u/DRIVE")), &mounts);
        assert!(!i.available);
        assert!(i.problem.unwrap().contains("not mounted"));
    }
}
