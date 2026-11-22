//! Asynchronous frame sink: the capture thread hands frames over (a bounded in-memory queue) and a
//! writer thread streams them to disk, so slow storage never throttles the camera (ADR 0012).
//!
//! The queue capacity is the RAM buffer. When it is full, `push` blocks (frames are never silently
//! dropped); a camera faster than the disk therefore runs at disk speed once the buffer is full.
//! On a write error the writer keeps draining (and discarding) so the capture side can never deadlock.

use crate::{write_fits, write_png, FitsMeta, SerWriter};
use rustflare_core::Frame;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Format {
    /// One SER file per burst (rolled over into `_partN` files at the filesystem's size limit).
    Ser,
    /// A folder of numbered 8/16-bit PNG files per burst.
    Png,
    /// A folder of numbered FITS files per burst (header: time, exposure, gain, instrument).
    Fits,
}

#[derive(Clone, Debug)]
pub struct BurstSpec {
    /// Path without extension: `<stem>.ser` (plus `_partN`), or the folder `<stem>/`.
    pub stem: PathBuf,
    pub format: Format,
    pub instrument: String,
    /// Largest file the filesystem can hold (FAT32: 4 GiB − 1); SER files roll over before reaching it.
    pub max_file_bytes: Option<u64>,
    /// FITS header values (only used for `Format::Fits`).
    pub fits_meta: FitsMeta,
}

#[derive(Clone, Debug)]
pub struct Finished {
    /// The `.ser` file (first part) or the PNG folder.
    pub path: PathBuf,
    pub frames: u32,
    pub parts: u32,
}

enum Msg {
    Begin(BurstSpec),
    Frame(Frame),
    End,
    Quit,
}

#[derive(Default)]
struct Shared {
    queued: AtomicUsize,
    written_frames: AtomicU64,
    written_bytes: AtomicU64,
    /// Frames thrown away because the RAM buffer was full and the policy is to drop (never counted as written).
    dropped: AtomicU64,
    /// Smoothed disk throughput in bytes/s, as measured around each frame write.
    rate: AtomicU64,
    error: Mutex<Option<String>>,
    finished: Mutex<Vec<Finished>>,
}

pub struct FrameSink {
    tx: SyncSender<Msg>,
    shared: Arc<Shared>,
    handle: Option<JoinHandle<()>>,
}

impl FrameSink {
    /// `capacity_frames` is the RAM buffer size in frames (at least 2).
    pub fn new(capacity_frames: usize) -> FrameSink {
        let (tx, rx) = sync_channel::<Msg>(capacity_frames.max(2));
        let shared = Arc::new(Shared::default());
        let sh = shared.clone();
        let handle = std::thread::Builder::new()
            .name("frame-writer".into())
            .spawn(move || {
                let mut active: Option<Active> = None;
                while let Ok(msg) = rx.recv() {
                    match msg {
                        Msg::Begin(spec) => active = Some(Active::new(spec)),
                        Msg::Frame(f) => {
                            sh.queued.fetch_sub(1, Ordering::Relaxed);
                            if sh.error.lock().unwrap().is_some() {
                                continue; // keep draining, discard
                            }
                            if let Some(a) = active.as_mut() {
                                let t = Instant::now();
                                let bytes = f.data.len() as u64;
                                match a.write(&f) {
                                    Ok(()) => {
                                        sh.written_frames.fetch_add(1, Ordering::Relaxed);
                                        sh.written_bytes.fetch_add(bytes, Ordering::Relaxed);
                                        let inst = bytes as f64 / t.elapsed().as_secs_f64().max(1e-6);
                                        let old = sh.rate.load(Ordering::Relaxed) as f64;
                                        let ema = if old == 0.0 { inst } else { 0.8 * old + 0.2 * inst };
                                        sh.rate.store(ema as u64, Ordering::Relaxed);
                                    }
                                    Err(e) => *sh.error.lock().unwrap() = Some(e),
                                }
                            }
                        }
                        Msg::End => {
                            if let Some(a) = active.take() {
                                match a.finish() {
                                    Ok((fin, rate)) => {
                                        sh.finished.lock().unwrap().push(fin);
                                        if rate > 0.0 {
                                            sh.rate.store(rate as u64, Ordering::Relaxed);
                                        }
                                    }
                                    Err(e) => {
                                        let mut g = sh.error.lock().unwrap();
                                        g.get_or_insert(e);
                                    }
                                }
                            }
                        }
                        Msg::Quit => break,
                    }
                }
            })
            .expect("spawn writer thread");
        FrameSink { tx, shared, handle: Some(handle) }
    }

    pub fn begin(&self, spec: BurstSpec) {
        let _ = self.tx.send(Msg::Begin(spec));
    }

    /// Queues a frame; blocks while the RAM buffer is full. Returns false if the writer has died.
    pub fn push(&self, frame: Frame) -> bool {
        self.shared.queued.fetch_add(1, Ordering::Relaxed);
        if self.tx.send(Msg::Frame(frame)).is_err() {
            self.shared.queued.fetch_sub(1, Ordering::Relaxed);
            return false;
        }
        true
    }

    /// Queues a frame without ever blocking: when the RAM buffer is full the frame is dropped and counted.
    /// Returns `Some(true)` if queued, `Some(false)` if dropped, `None` if the writer has died.
    pub fn try_push(&self, frame: Frame) -> Option<bool> {
        self.shared.queued.fetch_add(1, Ordering::Relaxed);
        match self.tx.try_send(Msg::Frame(frame)) {
            Ok(()) => Some(true),
            Err(std::sync::mpsc::TrySendError::Full(_)) => {
                self.shared.queued.fetch_sub(1, Ordering::Relaxed);
                self.shared.dropped.fetch_add(1, Ordering::Relaxed);
                Some(false)
            }
            Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                self.shared.queued.fetch_sub(1, Ordering::Relaxed);
                None
            }
        }
    }

    /// Ends the current burst; its file is finalized after the queued frames are written.
    pub fn end(&self) {
        let _ = self.tx.send(Msg::End);
    }

    /// Asks the writer to exit once everything queued has been written.
    pub fn close(&self) {
        let _ = self.tx.send(Msg::Quit);
    }

    pub fn queued(&self) -> usize { self.shared.queued.load(Ordering::Relaxed) }
    pub fn dropped(&self) -> u64 { self.shared.dropped.load(Ordering::Relaxed) }
    pub fn written_frames(&self) -> u64 { self.shared.written_frames.load(Ordering::Relaxed) }
    pub fn written_bytes(&self) -> u64 { self.shared.written_bytes.load(Ordering::Relaxed) }
    pub fn rate_bytes_per_s(&self) -> f64 { self.shared.rate.load(Ordering::Relaxed) as f64 }
    pub fn error(&self) -> Option<String> { self.shared.error.lock().unwrap().clone() }
    pub fn take_finished(&self) -> Vec<Finished> { std::mem::take(&mut *self.shared.finished.lock().unwrap()) }

    /// True once the writer thread has exited (after `close`).
    pub fn is_finished(&self) -> bool {
        self.handle.as_ref().map_or(true, |h| h.is_finished())
    }
}

impl Drop for FrameSink {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Quit);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Forces everything written so far onto the storage medium. Linux: only the filesystem holding `dir`
/// (`syncfs`), so a USB stick is flushed without waiting for unrelated writes elsewhere; elsewhere the whole
/// system (`sync`).
fn flush_filesystem(dir: &std::path::Path) {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::io::AsRawFd;
        if let Ok(f) = std::fs::File::open(dir) {
            // SAFETY: the descriptor is valid for the duration of the call.
            if unsafe { libc::syncfs(f.as_raw_fd()) } == 0 {
                return;
            }
        }
    }
    let _ = dir;
    // SAFETY: sync(2) has no preconditions.
    unsafe { libc::sync() };
}

struct Active {
    spec: BurstSpec,
    ser: Option<SerWriter>,
    frames: u32,
    parts: u32,
    bytes: u64,
    /// When the first frame of this burst reached the writer.
    started: Option<Instant>,
}

impl Active {
    fn new(spec: BurstSpec) -> Active {
        Active { spec, ser: None, frames: 0, parts: 0, bytes: 0, started: None }
    }

    fn part_path(&self, part: u32) -> PathBuf {
        let mut name = self.spec.stem.file_name().unwrap_or_default().to_os_string();
        if part > 1 {
            name.push(format!("_part{part}"));
        }
        name.push(".ser");
        self.spec.stem.with_file_name(name)
    }

    fn write(&mut self, f: &Frame) -> Result<(), String> {
        self.started.get_or_insert_with(Instant::now);
        self.bytes += f.data.len() as u64;
        match self.spec.format {
            Format::Png => {
                std::fs::create_dir_all(&self.spec.stem).map_err(|e| format!("{}: {e}", self.spec.stem.display()))?;
                let p = self.spec.stem.join(format!("frame_{:06}.png", self.frames + 1));
                write_png(&p, f).map_err(|e| e.to_string())?;
            }
            Format::Fits => {
                std::fs::create_dir_all(&self.spec.stem).map_err(|e| format!("{}: {e}", self.spec.stem.display()))?;
                let p = self.spec.stem.join(format!("frame_{:06}.fits", self.frames + 1));
                write_fits(&p, f, &self.spec.fits_meta).map_err(|e| e.to_string())?;
            }
            Format::Ser => {
                if let (Some(w), Some(max)) = (self.ser.as_ref(), self.spec.max_file_bytes) {
                    if w.projected_size_with_next() > max {
                        self.ser.take().unwrap().finalize().map_err(|e| e.to_string())?;
                    }
                }
                if self.ser.is_none() {
                    self.parts += 1;
                    let p = self.part_path(self.parts);
                    if let Some(dir) = p.parent() {
                        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                    }
                    let name = if self.parts > 1 { format!("{} (part {})", self.spec.instrument, self.parts) } else { self.spec.instrument.clone() };
                    self.ser = Some(SerWriter::create_color(&p, f.width, f.height, f.format, f.bayer, &name, "Rustflare").map_err(|e| e.to_string())?);
                }
                self.ser.as_mut().unwrap().write_frame(f).map_err(|e| e.to_string())?;
            }
        }
        self.frames += 1;
        Ok(())
    }

    /// Finalizes the burst and forces it onto the storage medium: "saved" must mean safe to unplug, and
    /// the returned throughput (bytes/s) is the *real* write speed, not the speed of filling the OS cache.
    fn finish(mut self) -> Result<(Finished, f64), String> {
        if let Some(w) = self.ser.take() {
            w.finalize().map_err(|e| e.to_string())?;
        }
        let dir = match self.spec.format {
            Format::Png | Format::Fits => self.spec.stem.clone(),
            Format::Ser => self.spec.stem.parent().map(|p| p.to_path_buf()).unwrap_or_default(),
        };
        flush_filesystem(&dir);
        let rate = self.started.map_or(0.0, |t| self.bytes as f64 / t.elapsed().as_secs_f64().max(1e-3));
        let path = match self.spec.format {
            Format::Ser => self.part_path(1),
            Format::Png | Format::Fits => self.spec.stem.clone(),
        };
        Ok((Finished { path, frames: self.frames, parts: self.parts.max(1) }, rate))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustflare_core::PixelFormat;
    use std::time::Duration;

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("rustflare-sink-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }
    fn frame() -> Frame { Frame::new(4, 4, PixelFormat::Mono16, vec![7; 32]) }
    fn wait_done(s: &FrameSink) {
        // a burst is reported finished only after its flush to the medium, which can take seconds on a busy runner
        for _ in 0..3000 {
            if s.queued() == 0 && !s.take_finished_peek() { return; }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    impl FrameSink {
        fn take_finished_peek(&self) -> bool { self.shared.finished.lock().unwrap().is_empty() }
    }

    #[test]
    fn buffers_then_writes_a_ser_burst() {
        let dir = tmp("ser");
        let sink = FrameSink::new(8);
        sink.begin(BurstSpec { stem: dir.join("b1"), format: Format::Ser, instrument: "cam".into(), max_file_bytes: None, fits_meta: Default::default() });
        for _ in 0..20 { assert!(sink.push(frame())); } // more than the 8-frame buffer: push blocks, never drops
        sink.end();
        wait_done(&sink);
        let fin = sink.take_finished();
        assert_eq!(fin.len(), 1);
        assert_eq!((fin[0].frames, fin[0].parts), (20, 1));
        assert_eq!(sink.written_frames(), 20);
        assert!(sink.error().is_none());
        let len = std::fs::metadata(dir.join("b1.ser")).unwrap().len();
        assert_eq!(len, 178 + 20 * (32 + 8));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rolls_over_at_the_file_size_limit() {
        let dir = tmp("roll");
        let sink = FrameSink::new(4);
        // limit fits header + 3 frames per file
        let max = 178 + 3 * (32 + 8);
        sink.begin(BurstSpec { stem: dir.join("b"), format: Format::Ser, instrument: "cam".into(), max_file_bytes: Some(max), fits_meta: Default::default() });
        for _ in 0..8 { sink.push(frame()); }
        sink.end();
        wait_done(&sink);
        let fin = sink.take_finished();
        assert_eq!((fin[0].frames, fin[0].parts), (8, 3));
        for f in ["b.ser", "b_part2.ser", "b_part3.ser"] {
            assert!(std::fs::metadata(dir.join(f)).unwrap().len() <= max, "{f}");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn png_burst_makes_a_folder_of_numbered_files() {
        let dir = tmp("png");
        let sink = FrameSink::new(4);
        sink.begin(BurstSpec { stem: dir.join("burst"), format: Format::Png, instrument: "cam".into(), max_file_bytes: None, fits_meta: Default::default() });
        for _ in 0..5 { sink.push(frame()); }
        sink.end();
        wait_done(&sink);
        assert_eq!(std::fs::read_dir(dir.join("burst")).unwrap().count(), 5);
        assert!(dir.join("burst/frame_000005.png").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_error_is_reported_and_never_deadlocks() {
        let sink = FrameSink::new(2);
        // a path below a regular file cannot be created
        let dir = tmp("err");
        let blocker = dir.join("file");
        std::fs::write(&blocker, b"x").unwrap();
        sink.begin(BurstSpec { stem: blocker.join("sub/b"), format: Format::Ser, instrument: "cam".into(), max_file_bytes: None, fits_meta: Default::default() });
        for _ in 0..50 { sink.push(frame()); } // would hang forever if the writer stopped consuming
        sink.end();
        for _ in 0..200 { if sink.queued() == 0 { break; } std::thread::sleep(Duration::from_millis(10)); }
        assert!(sink.error().is_some());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn fits_burst_writes_numbered_files() {
        let dir = std::env::temp_dir().join(format!("rf-fits-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let sink = FrameSink::new(4);
        sink.begin(BurstSpec { stem: dir.join("b"), format: Format::Fits, instrument: "cam".into(), max_file_bytes: None, fits_meta: FitsMeta { exposure_us: Some(1000.0), ..Default::default() } });
        for _ in 0..3 {
            assert!(sink.push(Frame::new(4, 4, rustflare_core::PixelFormat::Mono16, vec![0; 32])));
        }
        sink.end();
        sink.close();
        while !sink.is_finished() { std::thread::sleep(std::time::Duration::from_millis(5)); }
        assert!(sink.error().is_none(), "{:?}", sink.error());
        let n = std::fs::read_dir(dir.join("b")).unwrap().filter(|e| e.as_ref().unwrap().path().extension().is_some_and(|x| x == "fits")).count();
        assert_eq!(n, 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn try_push_drops_instead_of_blocking_when_full() {
        let dir = std::env::temp_dir().join(format!("rf-drop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // no Begin message: the writer discards frames but a tiny queue still fills while it is parked
        let sink = FrameSink::new(2);
        sink.begin(BurstSpec { stem: dir.join("b"), format: Format::Ser, instrument: "cam".into(), max_file_bytes: None, fits_meta: Default::default() });
        let big = || Frame::new(512, 512, rustflare_core::PixelFormat::Mono16, vec![0; 512 * 512 * 2]);
        let mut dropped = 0;
        for _ in 0..200 {
            if sink.try_push(big()) == Some(false) { dropped += 1; }
        }
        assert_eq!(sink.dropped(), dropped);
        sink.end();
        sink.close();
        while !sink.is_finished() { std::thread::sleep(std::time::Duration::from_millis(5)); }
        let _ = std::fs::remove_dir_all(&dir);
        assert!(dropped > 0 || sink.written_frames() == 200, "either some were dropped or the writer kept up");
    }
}
