//! SER video writer (the de-facto format for planetary/solar capture).
//!
//! Layout follows Vidoxide's `output/ser.rs` (MIT) — including its note that the
//! header's endianness flag is 0 for little-endian pixel data in real-world
//! files, contrary to the published spec — plus the per-frame timestamp trailer
//! that Vidoxide left as a TODO.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use rustflare_core::{Error, Frame, PixelFormat, Result};

const HEADER_LEN: usize = 178;
const FRAME_COUNT_OFFSET: u64 = 38;
/// The header's frame count is brought up to date every this many frames, so a file cut short (drive pulled, power lost)
/// still declares the frames it holds and opens in a stacker; only the buffer since the last checkpoint is at risk.
const CHECKPOINT_FRAMES: usize = 50;
/// 100 ns ticks between 0001-01-01 and 1970-01-01.
const EPOCH_TICKS: u64 = 621_355_968_000_000_000;

fn ticks(t: SystemTime) -> u64 {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    EPOCH_TICKS + d.as_secs() * 10_000_000 + d.subsec_nanos() as u64 / 100
}

fn fixed<const N: usize>(s: &str) -> [u8; N] {
    let mut a = [0u8; N];
    let b = s.as_bytes();
    let n = b.len().min(N);
    a[..n].copy_from_slice(&b[..n]);
    a
}

/// Whether SER files are written with `O_DIRECT` (bypassing the kernel's page cache), set by the recorder from its setting. See `Direct`.
static DIRECT: AtomicBool = AtomicBool::new(false);
pub fn set_direct_io(on: bool) { DIRECT.store(on, Ordering::Relaxed) }
pub fn direct_io() -> bool { DIRECT.load(Ordering::Relaxed) }

/// Whether a file in `dir` can be written with `O_DIRECT` (not every filesystem allows it). Creates and removes a small file.
pub fn direct_io_supported(dir: &Path) -> std::result::Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let probe = dir.join(format!(".rustflare-direct-test-{}", std::process::id()));
        let r = Direct::open(&probe).and_then(|mut d| {
            d.append(&[0u8; BLOCK])?;
            d.flush_tail()
        });
        let _ = std::fs::remove_file(&probe);
        r.map_err(|e| e.to_string())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = dir;
        Err("direct writes need Linux".into())
    }
}

const BLOCK: usize = 4096;
/// Frames are collected in a buffer of this size and written to the drive in one aligned write.
const DIRECT_CHUNK: usize = 4 << 20;

/// A heap buffer aligned to `BLOCK`, as `O_DIRECT` requires.
struct Aligned {
    ptr: *mut u8,
    cap: usize,
}
unsafe impl Send for Aligned {}
impl Aligned {
    fn new(cap: usize) -> Aligned {
        let layout = std::alloc::Layout::from_size_align(cap, BLOCK).expect("layout");
        let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
        assert!(!ptr.is_null(), "out of memory for the write buffer");
        Aligned { ptr, cap }
    }
    fn bytes(&self) -> &[u8] { unsafe { std::slice::from_raw_parts(self.ptr, self.cap) } }
    fn bytes_mut(&mut self) -> &mut [u8] { unsafe { std::slice::from_raw_parts_mut(self.ptr, self.cap) } }
}
impl Drop for Aligned {
    fn drop(&mut self) { unsafe { std::alloc::dealloc(self.ptr, std::alloc::Layout::from_size_align(self.cap, BLOCK).unwrap()) } }
}

/// A file written with `O_DIRECT`: nothing goes through the page cache, so recording a few gigabytes does not fill memory with
/// frames waiting for a slow USB stick, which is what the frame buffer's memory competes with (issue #86). Frames are collected in
/// an aligned 4 MB buffer and written to the drive in whole chunks; the header count (which sits in the first block) is rewritten
/// with an aligned write of that block; the last partial chunk is padded to a block and the file cut to its true length.
#[cfg(target_os = "linux")]
struct Direct {
    f: File,
    stage: Aligned,
    len: usize,
    /// Bytes already on the drive (a multiple of `DIRECT_CHUNK`) and bytes appended in all.
    written: u64,
    total: u64,
    /// The first block of the file as written, to rewrite it with a new frame count.
    blk0: Aligned,
}

#[cfg(target_os = "linux")]
impl Direct {
    fn open(path: &Path) -> std::io::Result<Direct> {
        use std::os::unix::fs::OpenOptionsExt;
        let f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).custom_flags(libc::O_DIRECT).open(path)?;
        Ok(Direct { f, stage: Aligned::new(DIRECT_CHUNK), len: 0, written: 0, total: 0, blk0: Aligned::new(BLOCK) })
    }

    fn append(&mut self, mut data: &[u8]) -> std::io::Result<()> {
        use std::os::unix::fs::FileExt;
        while !data.is_empty() {
            let n = (DIRECT_CHUNK - self.len).min(data.len());
            let at = self.len;
            self.stage.bytes_mut()[at..at + n].copy_from_slice(&data[..n]);
            if self.total < BLOCK as u64 {
                let from = self.total as usize;
                let k = n.min(BLOCK - from);
                self.blk0.bytes_mut()[from..from + k].copy_from_slice(&data[..k]);
            }
            self.len += n;
            self.total += n as u64;
            data = &data[n..];
            if self.len == DIRECT_CHUNK {
                self.f.write_all_at(self.stage.bytes(), self.written)?;
                self.written += DIRECT_CHUNK as u64;
                self.len = 0;
            }
        }
        Ok(())
    }

    /// Rewrites the frame count in the header.
    fn patch_count(&mut self, n: u32) -> std::io::Result<()> {
        use std::os::unix::fs::FileExt;
        let b = n.to_le_bytes();
        self.blk0.bytes_mut()[FRAME_COUNT_OFFSET as usize..FRAME_COUNT_OFFSET as usize + 4].copy_from_slice(&b);
        if self.written > 0 {
            self.f.write_all_at(self.blk0.bytes(), 0)
        } else {
            self.stage.bytes_mut()[FRAME_COUNT_OFFSET as usize..FRAME_COUNT_OFFSET as usize + 4].copy_from_slice(&b);
            Ok(())
        }
    }

    /// Writes what is left in the buffer (padded to a block) and cuts the file to its true length.
    fn flush_tail(&mut self) -> std::io::Result<()> {
        use std::os::unix::fs::FileExt;
        if self.len > 0 {
            let padded = self.len.div_ceil(BLOCK) * BLOCK;
            let at = self.len;
            self.stage.bytes_mut()[at..padded].fill(0);
            self.f.write_all_at(&self.stage.bytes()[..padded], self.written)?;
        }
        self.f.set_len(self.total)?;
        self.f.sync_data()
    }
}

/// Where a `SerWriter` puts its bytes.
enum Out {
    Buffered(BufWriter<File>),
    #[cfg(target_os = "linux")]
    Direct(Direct),
}

impl Out {
    fn append(&mut self, b: &[u8]) -> std::io::Result<()> {
        match self {
            Out::Buffered(w) => w.write_all(b),
            #[cfg(target_os = "linux")]
            Out::Direct(d) => d.append(b),
        }
    }
}

/// Written data is handed to the drive in chunks of this size while it is still being written (see `SerWriter::write_behind`).
const WRITE_BEHIND_BYTES: u64 = 64 << 20;

pub struct SerWriter {
    out: Out,
    format: (u32, u32, PixelFormat),
    stamps: Vec<u64>,
    /// Write-behind: the file offset up to which writeback has been started, and the chunk before it that is waited for and dropped.
    started_to: u64,
    dropped_to: u64,
}

impl SerWriter {
    pub fn create(path: &Path, width: u32, height: u32, format: PixelFormat, instrument: &str, telescope: &str) -> Result<SerWriter> {
        Self::create_color(path, width, height, format, None, instrument, telescope)
    }

    /// Like `create`, for a raw Bayer mosaic: the header carries the pattern so stacking software demosaics it.
    pub fn create_color(path: &Path, width: u32, height: u32, format: PixelFormat, bayer: Option<rustflare_core::BayerPattern>, instrument: &str, telescope: &str) -> Result<SerWriter> {
        Self::create_with(path, width, height, format, bayer, instrument, telescope, direct_io())
    }

    /// `create_color` with the choice of direct writes (`O_DIRECT`, Linux only: an error elsewhere or where the filesystem refuses it).
    pub fn create_with(path: &Path, width: u32, height: u32, format: PixelFormat, bayer: Option<rustflare_core::BayerPattern>, instrument: &str, telescope: &str, direct: bool) -> Result<SerWriter> {
        #[cfg(target_os = "linux")]
        let mut out = if direct { Out::Direct(Direct::open(path)?) } else { Out::Buffered(BufWriter::new(File::create(path)?)) };
        #[cfg(not(target_os = "linux"))]
        let mut out = {
            if direct {
                return Err(Error::Invalid("direct writes need Linux".into()));
            }
            Out::Buffered(BufWriter::new(File::create(path)?))
        };
        let now = ticks(SystemTime::now());
        let mut h = Vec::with_capacity(HEADER_LEN);
        h.extend_from_slice(&fixed::<14>("LUCAM-RECORDER"));
        h.extend_from_slice(&0u32.to_le_bytes()); // camera series id
        h.extend_from_slice(&bayer.map_or(0, |b| b.ser_color_id()).to_le_bytes()); // colour id: 0 mono, 8..11 Bayer
        h.extend_from_slice(&0u32.to_le_bytes()); // endianness flag, see module docs
        h.extend_from_slice(&width.to_le_bytes());
        h.extend_from_slice(&height.to_le_bytes());
        h.extend_from_slice(&((format.bytes_per_pixel() * 8) as u32).to_le_bytes());
        h.extend_from_slice(&0u32.to_le_bytes()); // frame count, patched on finalize
        h.extend_from_slice(&fixed::<40>("")); // observer
        h.extend_from_slice(&fixed::<40>(instrument));
        h.extend_from_slice(&fixed::<40>(telescope));
        h.extend_from_slice(&now.to_le_bytes()); // local time (we only know UTC)
        h.extend_from_slice(&now.to_le_bytes()); // UTC
        debug_assert_eq!(h.len(), HEADER_LEN);
        out.append(&h)?;
        Ok(SerWriter { out, format: (width, height, format), stamps: Vec::new(), started_to: HEADER_LEN as u64, dropped_to: HEADER_LEN as u64 })
    }

    pub fn write_frame(&mut self, f: &Frame) -> Result<()> {
        self.write_frame_data(f.width, f.height, f.format, &f.data, f.captured_at)
    }

    /// The same from the frame's bytes and metadata, for a frame that lives in shared memory and should not be copied first.
    pub fn write_frame_data(&mut self, width: u32, height: u32, format: PixelFormat, data: &[u8], captured_at: SystemTime) -> Result<()> {
        if (width, height, format) != self.format {
            return Err(Error::Invalid(format!(
                "frame {}x{} {:?} differs from sequence format {:?}", width, height, format, self.format
            )));
        }
        self.out.append(data)?;
        self.stamps.push(ticks(captured_at));
        if self.stamps.len() % CHECKPOINT_FRAMES == 0 {
            self.checkpoint()?;
            self.write_behind();
        }
        Ok(())
    }

    /// Smooths the drive's workload (issue #86). Left alone, the kernel collects gigabytes of written frames in memory and then
    /// writes them out in one flood, which on a USB stick on FAT32 starves everything else on the machine for a second or two
    /// (the camera's USB transfers, the preview). So every 64 MB: start writing that chunk to the drive now (without waiting for
    /// it), wait for the chunk before it, and drop that one from the page cache, which also keeps memory free for the frame buffer.
    /// Best effort: a failure here changes nothing about the data, which the ordinary writes still carry.
    fn write_behind(&mut self) {
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd;
            let Out::Buffered(bw) = &self.out else { return };
            let (w, h, f) = self.format;
            let end = HEADER_LEN as u64 + self.stamps.len() as u64 * w as u64 * h as u64 * f.bytes_per_pixel() as u64;
            if end < self.started_to + WRITE_BEHIND_BYTES {
                return;
            }
            let fd = bw.get_ref().as_raw_fd();
            let (from, to) = (self.started_to, end);
            unsafe {
                libc::sync_file_range(fd, from as i64, (to - from) as i64, libc::SYNC_FILE_RANGE_WRITE);
                if self.dropped_to < from {
                    libc::sync_file_range(fd, self.dropped_to as i64, (from - self.dropped_to) as i64, libc::SYNC_FILE_RANGE_WAIT_BEFORE | libc::SYNC_FILE_RANGE_WRITE | libc::SYNC_FILE_RANGE_WAIT_AFTER);
                    libc::posix_fadvise(fd, self.dropped_to as i64, (from - self.dropped_to) as i64, libc::POSIX_FADV_DONTNEED);
                }
            }
            self.dropped_to = if self.dropped_to < from { from } else { self.dropped_to };
            self.started_to = to;
        }
    }

    /// Writes the current frame count into the header. With direct writes only the frames already on the drive are counted.
    fn checkpoint(&mut self) -> Result<()> {
        let (w, h, f) = self.format;
        let fsize = w as u64 * h as u64 * f.bytes_per_pixel() as u64;
        match &mut self.out {
            Out::Buffered(bw) => {
                let end = HEADER_LEN as u64 + self.stamps.len() as u64 * fsize;
                bw.flush()?;
                bw.seek(SeekFrom::Start(FRAME_COUNT_OFFSET))?;
                bw.write_all(&(self.stamps.len() as u32).to_le_bytes())?;
                bw.flush()?;
                bw.seek(SeekFrom::Start(end))?;
            }
            #[cfg(target_os = "linux")]
            Out::Direct(d) => {
                let on_drive = (d.written.saturating_sub(HEADER_LEN as u64) / fsize.max(1)).min(self.stamps.len() as u64) as u32;
                if on_drive > 0 {
                    d.patch_count(on_drive)?;
                }
            }
        }
        Ok(())
    }

    pub fn frame_count(&self) -> u32 { self.stamps.len() as u32 }

    /// Size the file will have if finalized now (header + frames + 8-byte timestamp per frame).
    pub fn projected_size(&self) -> u64 {
        let (w, h, f) = self.format;
        HEADER_LEN as u64 + self.stamps.len() as u64 * (w as u64 * h as u64 * f.bytes_per_pixel() as u64 + 8)
    }

    /// Size after one more frame is added.
    pub fn projected_size_with_next(&self) -> u64 {
        let (w, h, f) = self.format;
        self.projected_size() + w as u64 * h as u64 * f.bytes_per_pixel() as u64 + 8
    }

    /// Writes the timestamp trailer and patches the frame count. Returns the frame count.
    pub fn finalize(mut self) -> Result<u32> {
        let n = self.stamps.len() as u32;
        for st in std::mem::take(&mut self.stamps) {
            self.out.append(&st.to_le_bytes())?;
        }
        match &mut self.out {
            Out::Buffered(bw) => {
                bw.seek(SeekFrom::Start(FRAME_COUNT_OFFSET))?;
                bw.write_all(&n.to_le_bytes())?;
                bw.flush()?;
            }
            #[cfg(target_os = "linux")]
            Out::Direct(d) => {
                d.patch_count(n)?;
                d.flush_tail()?;
            }
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_cut_short_still_declares_the_frames_up_to_the_last_checkpoint() {
        let path = std::env::temp_dir().join(format!("rustflare-cut-{}.ser", std::process::id()));
        let mut w = SerWriter::create(&path, 4, 2, PixelFormat::Mono16, "cam", "scope").unwrap();
        let f = Frame::new(4, 2, PixelFormat::Mono16, vec![7u8; 16]);
        for _ in 0..130 { w.write_frame(&f).unwrap(); }
        if let Out::Buffered(bw) = &mut w.out { bw.flush().unwrap(); }
        std::mem::forget(w); // the process dies here: no finalize, no timestamp trailer
        let bytes = std::fs::read(&path).unwrap();
        let count = u32::from_le_bytes(bytes[38..42].try_into().unwrap());
        assert_eq!(count, 100, "header count after 130 frames with a checkpoint every 50");
        assert!(bytes.len() >= HEADER_LEN + 100 * 16, "all 100 declared frames are in the file");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn writes_valid_ser_layout() {
        let path = std::env::temp_dir().join(format!("rustflare-test-{}.ser", std::process::id()));
        let mut w = SerWriter::create(&path, 4, 2, PixelFormat::Mono16, "cam", "scope").unwrap();
        for _ in 0..3 {
            w.write_frame(&Frame::new(4, 2, PixelFormat::Mono16, vec![1; 16])).unwrap();
        }
        assert!(w.write_frame(&Frame::new(2, 2, PixelFormat::Mono16, vec![0; 8])).is_err());
        assert_eq!(w.projected_size(), (HEADER_LEN + 3 * 16 + 3 * 8) as u64);
        assert_eq!(w.finalize().unwrap(), 3);
        let b = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(b.len(), HEADER_LEN + 3 * 16 + 3 * 8);
        assert_eq!(&b[..14], b"LUCAM-RECORDER");
        assert_eq!(u32::from_le_bytes(b[38..42].try_into().unwrap()), 3);
        assert_eq!(u32::from_le_bytes(b[26..30].try_into().unwrap()), 4); // width
        assert_eq!(u32::from_le_bytes(b[34..38].try_into().unwrap()), 16); // depth
    }

    #[test]
    fn bayer_pattern_goes_into_the_colour_id() {
        let dir = std::env::temp_dir().join(format!("rf-ser-bayer-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("c.ser");
        let mut w = SerWriter::create_color(&path, 4, 2, PixelFormat::Mono8, Some(rustflare_core::BayerPattern::Gbrg), "cam", "scope").unwrap();
        w.write_frame(&Frame::new(4, 2, PixelFormat::Mono8, vec![1; 8])).unwrap();
        w.finalize().unwrap();
        let b = std::fs::read(&path).unwrap();
        assert_eq!(u32::from_le_bytes(b[18..22].try_into().unwrap()), 10);
        std::fs::remove_file(&path).unwrap();
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn a_long_recording_with_write_behind_is_still_a_valid_file() {
        // more than two write-behind chunks (64 MB each) of 2 MB frames
        let d = std::env::temp_dir().join(format!("rf-ser-wb-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("long.ser");
        let mut w = SerWriter::create(&p, 1024, 1024, PixelFormat::Mono16, "cam", "scope").unwrap();
        let n = 160u32;
        for i in 0..n {
            let f = Frame::new(1024, 1024, PixelFormat::Mono16, vec![(i % 251) as u8; 1024 * 1024 * 2]);
            w.write_frame(&f).unwrap();
        }
        assert_eq!(w.finalize().unwrap(), n);
        let b = std::fs::read(&p).unwrap();
        assert_eq!(b.len() as u64, 178 + n as u64 * (2 * 1024 * 1024 + 8));
        assert_eq!(u32::from_le_bytes(b[38..42].try_into().unwrap()), n);
        for i in [0u32, 70, 159] {
            let at = 178 + i as usize * 2 * 1024 * 1024;
            assert!(b[at..at + 2 * 1024 * 1024].iter().all(|v| *v == (i % 251) as u8), "frame {i}");
        }
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn direct_writes_give_the_same_file_as_buffered_ones_and_survive_a_cut() {
        let d = std::env::temp_dir().join(format!("rf-ser-direct-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        if let Err(e) = direct_io_supported(&d) {
            eprintln!("skipped: this filesystem does not allow O_DIRECT ({e})");
            let _ = std::fs::remove_dir_all(&d);
            return;
        }
        let write = |name: &str, direct: bool, n: u32| {
            let p = d.join(name);
            let mut w = SerWriter::create_with(&p, 1000, 777, PixelFormat::Mono16, None, "cam", "scope", direct).unwrap(); // 1.5 MB frames: chunks do not line up with frames
            for i in 0..n {
                let mut data = vec![(i % 251) as u8; 1000 * 777 * 2];
                data[0] = i as u8;
                w.write_frame(&Frame::new(1000, 777, PixelFormat::Mono16, data)).unwrap();
            }
            assert_eq!(w.finalize().unwrap(), n);
            std::fs::read(&p).unwrap()
        };
        for n in [1u32, 2, 9, 11] {
            let (a, b) = (write("b.ser", false, n), write("d.ser", true, n));
            assert_eq!(a.len(), b.len(), "{n} frames: same length");
            // the header's creation times (bytes 162 to 178) and the timestamp trailer differ between two writes: compare the rest
            let frames_end = HEADER_LEN + n as usize * 1000 * 777 * 2;
            assert_eq!(a[..162], b[..162], "{n} frames: same header");
            assert_eq!(a[HEADER_LEN..frames_end], b[HEADER_LEN..frames_end], "{n} frames: same frame bytes");
            assert_eq!(u32::from_le_bytes(b[38..42].try_into().unwrap()), n);
        }
        // cut short: only whole chunks are on the drive, and the header declares only the frames among them
        let p = d.join("cut.ser");
        let mut w = SerWriter::create_with(&p, 1000, 777, PixelFormat::Mono16, None, "cam", "scope", true).unwrap();
        for i in 0..110u32 {
            w.write_frame(&Frame::new(1000, 777, PixelFormat::Mono16, vec![i as u8; 1000 * 777 * 2])).unwrap();
        }
        std::mem::forget(w);
        let b = std::fs::read(&p).unwrap();
        let declared = u32::from_le_bytes(b[38..42].try_into().unwrap()) as usize;
        assert!(declared >= 50 && declared <= 110, "{declared}");
        assert!(b.len() >= HEADER_LEN + declared * 1000 * 777 * 2, "every declared frame is in the file");
        let _ = std::fs::remove_dir_all(&d);
    }
}
