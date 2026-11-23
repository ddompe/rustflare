//! The shared memory itself: creation, mapping, and nothing else. Linux uses `memfd_create`; other Unixes an unlinked temporary file
//! (for development: the ring then lives on disk).

use std::os::fd::{OwnedFd, RawFd};
#[cfg(target_os = "linux")]
use std::os::fd::FromRawFd;

pub struct Mapping {
    ptr: *mut u8,
    len: usize,
}

// The memory is shared on purpose and all access to the shared parts goes through atomics (see lib.rs).
unsafe impl Send for Mapping {}
unsafe impl Sync for Mapping {}

impl Mapping {
    pub fn ptr(&self) -> *mut u8 { self.ptr }
    pub fn len(&self) -> usize { self.len }

    pub fn create(len: usize) -> std::io::Result<(OwnedFd, Mapping)> {
        let fd = create_fd()?;
        if unsafe { libc::ftruncate(std::os::fd::AsRawFd::as_raw_fd(&fd), len as libc::off_t) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let m = Mapping::map(std::os::fd::AsRawFd::as_raw_fd(&fd), len)?;
        Ok((fd, m))
    }

    pub fn from_fd(fd: RawFd) -> std::io::Result<Mapping> {
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        if unsafe { libc::fstat(fd, &mut st) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Mapping::map(fd, st.st_size as usize)
    }

    fn map(fd: RawFd, len: usize) -> std::io::Result<Mapping> {
        let p = unsafe { libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, 0) };
        if p == libc::MAP_FAILED {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Mapping { ptr: p as *mut u8, len })
    }
}

impl Drop for Mapping {
    fn drop(&mut self) { unsafe { libc::munmap(self.ptr as *mut libc::c_void, self.len); } }
}

#[cfg(target_os = "linux")]
fn create_fd() -> std::io::Result<OwnedFd> {
    let name = b"rustflare-frames\0";
    let fd = unsafe { libc::memfd_create(name.as_ptr() as *const libc::c_char, libc::MFD_CLOEXEC) };
    if fd < 0 { return Err(std::io::Error::last_os_error()); }
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

#[cfg(not(target_os = "linux"))]
fn create_fd() -> std::io::Result<OwnedFd> {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!("rustflare-ring-{}-{}-{}", std::process::id(), COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)));
    let f = std::fs::OpenOptions::new().read(true).write(true).create_new(true).open(&path)?;
    std::fs::remove_file(&path)?;
    Ok(f.into())
}
