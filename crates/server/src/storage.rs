//! Where captures are stored: selectable at run time (e.g. a USB drive), persisted, and guarded
//! against the drive being removed, full, or on a filesystem with a file-size limit (ADR 0011).

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Refuse to start a capture with less free space than this.
pub const MIN_FREE_BYTES: u64 = 200 * 1024 * 1024;
/// Stop a running capture when free space drops below this.
pub const STOP_FREE_BYTES: u64 = 50 * 1024 * 1024;
const FAT_MAX_FILE: u64 = 4 * 1024 * 1024 * 1024 - 1;

#[derive(Clone, Debug, PartialEq)]
pub struct Mount {
    pub device: String,
    pub point: PathBuf,
    pub fstype: String,
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
            Some(Mount { device: unescape(f.next()?), point: PathBuf::from(unescape(f.next()?)), fstype: f.next()?.to_string() })
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
        .map(|d| d.flatten().map(|e| Mount { device: String::new(), point: e.path(), fstype: "unknown".into() }).collect())
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
}

struct State {
    dir: PathBuf,
    /// Mount point the directory was on when selected; it must still be mounted to use the directory.
    required_mount: Option<PathBuf>,
}

pub struct Storage {
    state: Mutex<State>,
    config_file: PathBuf,
}

impl Storage {
    /// Uses the saved choice in `config_file` if there is one, otherwise `default_dir`.
    pub fn load(config_file: PathBuf, default_dir: PathBuf) -> Storage {
        let saved = crate::config::read(&config_file).get("data_dir").and_then(|v| v.as_str()).map(PathBuf::from);
        let (dir, from_saved) = match saved {
            Some(d) => (d, true),
            None => (default_dir, false),
        };
        if from_saved {
            log::info!("storage: using saved choice {} (from {}; overrides --data-dir)", dir.display(), config_file.display());
        }
        let required_mount = containing_mount(&dir, &read_mounts()).map(|m| m.point.clone());
        if !dir.exists() && required_mount.is_none() {
            let _ = std::fs::create_dir_all(&dir);
        }
        Storage { state: Mutex::new(State { dir, required_mount }), config_file }
    }

    pub fn config_file(&self) -> &Path {
        &self.config_file
    }

    pub fn dir(&self) -> PathBuf {
        self.state.lock().unwrap().dir.clone()
    }

    pub fn info(&self) -> Info {
        let s = self.state.lock().unwrap();
        describe(&s.dir, s.required_mount.as_deref(), &read_mounts())
    }

    /// Fails with a user-facing message unless captures can safely be written now.
    pub fn ensure_ready(&self) -> Result<PathBuf, String> {
        let i = self.info();
        if !i.available {
            return Err(format!("storage not available: {}", i.problem.unwrap_or_default()));
        }
        if i.free.is_some_and(|f| f < MIN_FREE_BYTES) {
            return Err(format!("storage almost full ({} MB free)", i.free.unwrap_or(0) / 1_000_000));
        }
        let dir = self.dir();
        writable(&dir).map_err(|e| format!("storage not writable: {e}{}", perm_hint_str(&e)))?;
        Ok(dir)
    }

    /// Cheap check used while recording.
    pub fn running_problem(&self) -> Option<String> {
        let i = self.info();
        if !i.available {
            return Some(format!("storage not available: {}", i.problem.unwrap_or_default()));
        }
        i.free.filter(|f| *f < STOP_FREE_BYTES).map(|f| format!("disk full ({} MB left)", f / 1_000_000))
    }

    /// Drives and other mounted volumes that could hold captures (target folder: `<mount>/rustflare`).
    pub fn candidates(&self) -> Vec<Info> {
        let mounts = read_mounts();
        mounts
            .iter()
            .filter(|m| ["/media", "/mnt", "/run/media", "/Volumes"].iter().any(|p| m.point.starts_with(p)))
            .map(|m| {
                // judged on the drive itself: the `rustflare` folder is created when selected
                let mut i = describe(&m.point, Some(&m.point), &mounts);
                i.path = m.point.join("rustflare").display().to_string();
                i
            })
            .collect()
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
            .map_err(|e| format!("could not unmount (is something still using the drive?): {e}"))?;
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
        let required_mount = containing_mount(&dir, &mounts).map(|m| m.point.clone());
        crate::config::set(&self.config_file, "data_dir", serde_json::json!(dir))?;
        let mut s = self.state.lock().unwrap();
        s.dir = dir;
        s.required_mount = required_mount;
        Ok(describe(&s.dir, s.required_mount.as_deref(), &mounts))
    }
}

fn perm_hint(e: &std::io::Error) -> &'static str {
    if e.kind() == std::io::ErrorKind::PermissionDenied { PERM_HINT } else { "" }
}
fn perm_hint_str(e: &str) -> &'static str {
    if e.to_lowercase().contains("permission denied") { PERM_HINT } else { "" }
}
const PERM_HINT: &str = " (permission denied: auto-mounted drives belong to your login user; run `sudo rustflare-user <your-login>` to run the service as that user)";

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

    #[test]
    fn removed_drive_is_unavailable() {
        let mounts = parse_mounts("/dev/a / ext4 rw 0 0\n");
        let i = describe(Path::new("/media/u/DRIVE/rustflare"), Some(Path::new("/media/u/DRIVE")), &mounts);
        assert!(!i.available);
        assert!(i.problem.unwrap().contains("not mounted"));
    }
}
