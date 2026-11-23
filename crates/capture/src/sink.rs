//! Asynchronous frame sink: the capture thread hands frames over (a ring of frame slots in shared memory, ADR 0019) and a
//! writer thread streams them to disk, so slow storage never throttles the camera (ADR 0012).
//!
//! The ring's slots are the RAM buffer. In wait mode, when it is full, `push` blocks (frames are never silently
//! dropped); a camera faster than the disk therefore runs at disk speed once the buffer is full. In drop mode the camera side never
//! waits and what the writer cannot keep up with is overwritten and counted.
//! On a write error the writer keeps draining (and discarding) so the capture side can never deadlock.

use crate::{write_fits, write_png, FitsMeta, SerWriter};
use rustflare_core::Frame;
use rustflare_ring::{Consumer, Full, FrameMeta, Geometry, Next, Producer, Ring};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, UNIX_EPOCH};

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
    /// Frames of this burst lost because the RAM buffer was full (drop mode): overwritten before the writer got to them. 0 in wait mode.
    pub dropped: u64,
    /// The burst's raw video was deliberately not written (its stack was made and nothing else was asked for): there is no file.
    pub skipped: bool,
}

/// What to do with the raw video of a burst whose writing was deferred until its stack existed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolution {
    /// The stack is made and the video is not wanted.
    SkipRaw,
    /// Write every frame (the stack failed, or the video is wanted as well).
    WriteAll,
    /// Write only these frames, by index in the burst (the best N % of the stacker's ranking).
    WriteSelected(Vec<u32>),
}

/// Control messages to the writer thread. Frames do not travel here: they are in the ring (see below).
enum Msg {
    /// The ring and the writer's place in it (sent before the first frame is committed).
    Ring(Consumer),
    Begin { id: u64, spec: BurstSpec, deferred: bool },
    /// The burst `id` has no frames after sequence number `last_seq` (0 if it has none).
    End { id: u64, last_seq: u64 },
    Resolve { id: u64, resolution: Resolution },
    Quit,
}

#[derive(Default)]
struct Shared {
    /// Frames pushed and not yet taken by the writer (the buffer's fill).
    queued: AtomicUsize,
    written_frames: AtomicU64,
    written_bytes: AtomicU64,
    /// Frames lost because the RAM buffer was full: refused (wait mode with `try_push`) or overwritten before the writer got to them
    /// (drop mode). Never counted as written.
    dropped: AtomicU64,
    /// Smoothed disk throughput in bytes/s, as measured around each frame write.
    rate: AtomicU64,
    error: Mutex<Option<String>>,
    finished: Mutex<Vec<Finished>>,
    /// The sequence numbers each burst owns, written by the producer at `begin` and `end` (before any frame of the burst is committed),
    /// so that a lost range of frames can be charged to the bursts it belonged to exactly.
    ranges: Mutex<Vec<BurstRange>>,
    /// Bursts whose frames stay in the ring after they are written (another process is stacking them): their memory is not released.
    keep: Mutex<std::collections::HashSet<u64>>,
    /// Tests only: holds the writer still so that losses can be provoked deterministically.
    #[cfg(test)]
    pause: std::sync::atomic::AtomicBool,
}

/// Frames `first..=last` of the ring belong to burst `id` (`last` is None while the burst is open).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BurstRange {
    id: u64,
    first: u64,
    last: Option<u64>,
}

/// How many of the frames `from..=to` belong to each burst: pure, so it can be tested on its own.
fn attribute(ranges: &[BurstRange], from: u64, to: u64) -> Vec<(u64, u64)> {
    ranges
        .iter()
        .filter_map(|r| {
            let (a, b) = (r.first.max(from), r.last.unwrap_or(u64::MAX).min(to));
            (a <= b).then(|| (r.id, b - a + 1))
        })
        .collect()
}

/// The producer's side of the ring, made when the first frame shows what the frames look like.
struct Feed {
    ring: Ring,
    fd: std::os::fd::OwnedFd,
    producer: Producer,
    geometry: Geometry,
}

/// Frames go camera thread, ring (shared memory, ADR 0019), writer thread, file. The ring is the RAM buffer: its slots are the
/// capacity. In the default (wait) mode the writer holds the producer back when the ring is full, so `push` blocks and no frame is lost;
/// in drop mode the writer is a best-effort consumer: the camera side never waits, and what the writer cannot keep up with is
/// overwritten and counted as dropped.
pub struct FrameSink {
    tx: Sender<Msg>,
    shared: Arc<Shared>,
    handle: Option<JoinHandle<()>>,
    capacity: usize,
    drop_when_full: bool,
    feed: Mutex<Option<Feed>>,
    burst_id: AtomicU64,
}

/// Geometry of a frame in the ring and back.
fn geometry_of(f: &Frame) -> Geometry {
    use rustflare_core::BayerPattern::*;
    Geometry {
        width: f.width,
        height: f.height,
        bits: if f.format == rustflare_core::PixelFormat::Mono16 { 16 } else { 8 },
        format: match f.bayer { None => 0, Some(Rggb) => 8, Some(Grbg) => 9, Some(Gbrg) => 10, Some(Bggr) => 11 },
    }
}

fn pixel_format(g: &Geometry) -> rustflare_core::PixelFormat {
    if g.bits > 8 { rustflare_core::PixelFormat::Mono16 } else { rustflare_core::PixelFormat::Mono8 }
}

fn bayer(g: &Geometry) -> Option<rustflare_core::BayerPattern> {
    use rustflare_core::BayerPattern::*;
    match g.format { 8 => Some(Rggb), 9 => Some(Grbg), 10 => Some(Gbrg), 11 => Some(Bggr), _ => None }
}

struct Pending {
    id: u64,
    spec: BurstSpec,
    /// Its frames are left in the ring until `Resolve` says what to write.
    deferred: bool,
    resolution: Option<Resolution>,
    /// The burst has been completed (its file finalised and reported); only its place in the queue is left.
    finished: bool,
    /// Sequence number of the burst's last frame, once the burst has ended.
    end: Option<u64>,
}

/// The writer thread's state.
struct Writer {
    sh: Arc<Shared>,
    consumer: Option<Consumer>,
    pending: VecDeque<Pending>,
    active: Option<(u64, Active)>,
    quit: bool,
    /// A private copy of a frame for a consumer that can be overwritten (drop mode): copied out, checked, then written.
    scratch: Vec<u8>,
    /// Frames lost so far, per burst, until the burst is reported.
    lost: std::collections::HashMap<u64, u64>,
    /// Whether the writer holds the producer back while it writes (wait mode).
    hold: bool,
}

/// What happened when the writer tried to write the frame it had chosen.
enum Outcome {
    Written(Result<(), String>, u64, Instant),
    /// An earlier write failed: the frame is discarded so the camera side can never deadlock.
    Discarded,
    /// The camera overwrote the frame while the writer was taking it (drop mode).
    Overwritten,
    /// The writer was lapped between choosing the frame and reading it: these frames were lost (and the cursor has moved).
    Lapped(u64, u64),
    /// Nothing to do after all (the frame is not visible yet).
    Retry,
}

impl Writer {
    fn on_msg(&mut self, m: Msg) {
        match m {
            Msg::Ring(c) => self.consumer = Some(c),
            Msg::Begin { id, spec, deferred } => self.pending.push_back(Pending { id, spec, deferred, resolution: None, finished: false, end: None }),
            Msg::Resolve { id, resolution } => {
                if let Some(p) = self.pending.iter_mut().find(|p| p.id == id) {
                    p.resolution = Some(resolution);
                }
            }
            Msg::End { id, last_seq } => {
                if let Some(p) = self.pending.iter_mut().find(|p| p.id == id) {
                    p.end = Some(last_seq);
                }
            }
            Msg::Quit => self.quit = true,
        }
    }

    /// Frames `from..=to` were lost (overwritten before the writer took them): counted in the total and charged to the bursts they
    /// belonged to, exactly, from the producer's table of sequence ranges.
    fn note_lost(&mut self, from: u64, to: u64) {
        if to < from {
            return;
        }
        self.sh.dropped.fetch_add(to - from + 1, Ordering::Relaxed);
        let owners = attribute(&self.sh.ranges.lock().unwrap(), from, to);
        for (id, n) in owners {
            *self.lost.entry(id).or_insert(0) += n;
        }
    }

    /// Takes a burst's lost-frame count (and its place in the producer's table) once it is reported.
    fn take_lost(&mut self, id: u64) -> u64 {
        self.sh.ranges.lock().unwrap().retain(|r| r.id != id);
        self.lost.remove(&id).unwrap_or(0)
    }

    fn finish_active(&mut self) {
        if let Some((id, mut a)) = self.active.take() {
            a.dropped = self.take_lost(id);
            if let Some(p) = self.pending.iter_mut().find(|p| p.id == id) {
                p.finished = true; // it must not be completed a second time when its End is reached
            }
            self.complete(a);
        }
    }

    fn complete(&mut self, a: Active) {
        match a.finish() {
            Ok((fin, rate)) => {
                self.sh.finished.lock().unwrap().push(fin);
                if rate > 0.0 {
                    self.sh.rate.store(rate as u64, Ordering::Relaxed);
                }
            }
            Err(e) => {
                let mut g = self.sh.error.lock().unwrap();
                g.get_or_insert(e);
            }
        }
    }

    /// Writes every frame that is ready. Returns whether anything happened and whether a frame is waiting for its burst's `Begin`.
    fn drain_ring(&mut self) -> (bool, bool) {
        let (mut progressed, mut need_control) = (false, false);
        loop {
            // which frame is next, and which burst does it belong to
            let (seq, burst) = {
                let Some(c) = self.consumer.as_mut() else { break };
                let before = c.read_seq();
                match c.peek() {
                    Ok(f) => (f.seq, f.burst_id),
                    Err(Next::Empty) => break,
                    Err(Next::Lapped(n)) => {
                        self.note_lost(before + 1, before + n);
                        self.sh.queued.fetch_sub(n as usize, Ordering::Relaxed);
                        progressed = true;
                        continue;
                    }
                }
            };
            // the burst must be the active one; a new burst means the previous one has had all its frames
            if self.active.as_ref().map(|(id, _)| *id) != Some(burst) {
                let Some(pos) = self.pending.iter().position(|p| p.id == burst) else {
                    need_control = true; // its Begin has not been read from the channel yet
                    break;
                };
                let spec = self.pending[pos].spec.clone();
                self.finish_active();
                self.active = Some((burst, Active::new(spec)));
            }
            // a burst whose raw video was deferred waits in the ring until it is resolved
            let (deferred, resolution, first_seq) = {
                let p = self.pending.iter().find(|p| p.id == burst);
                let first = self.sh.ranges.lock().unwrap().iter().find(|r| r.id == burst).map_or(1, |r| r.first);
                (p.map_or(false, |p| p.deferred), p.and_then(|p| p.resolution.clone()), first)
            };
            if deferred {
                match &resolution {
                    None => {
                        // let go of the camera while waiting for the decision: this burst is protected by headroom, not by a hold
                        self.consumer.as_ref().unwrap().set_must_not_lose(false);
                        break;
                    }
                    Some(Resolution::SkipRaw) => {
                        if !self.skip_burst(burst) {
                            break; // the burst is still open: its frames are still arriving
                        }
                        progressed = true;
                        continue;
                    }
                    Some(Resolution::WriteAll) | Some(Resolution::WriteSelected(_)) => {
                        self.consumer.as_ref().unwrap().set_must_not_lose(self.hold);
                        if let Some(Resolution::WriteSelected(idx)) = &resolution {
                            if !idx.contains(&((seq - first_seq) as u32)) {
                                let c = self.consumer.as_ref().unwrap();
                                c.advance(seq);
                                self.sh.queued.fetch_sub(1, Ordering::Relaxed);
                                progressed = true;
                                continue; // not one of the chosen frames
                            }
                        }
                    }
                }
            }
            // write it; only fields are touched while the frame is borrowed
            let outcome = {
                let c = self.consumer.as_mut().unwrap();
                let g = c.ring().geometry();
                let hold = c.must_not_lose(); // a held frame cannot be overwritten, so it is written straight from the ring
                let before = c.read_seq();
                match c.peek() {
                    Ok(f) if f.seq == seq => {
                        if self.sh.error.lock().unwrap().is_some() {
                            Outcome::Discarded
                        } else {
                            let t = Instant::now();
                            let bytes = f.data.len() as u64;
                            let captured_at = UNIX_EPOCH + Duration::from_nanos(f.ts_ns);
                            let a = &mut self.active.as_mut().unwrap().1;
                            if hold {
                                let fd = FrameData { width: g.width, height: g.height, format: pixel_format(&g), bayer: bayer(&g), data: f.data, captured_at };
                                Outcome::Written(a.write(&fd), bytes, t)
                            } else {
                                self.scratch.clear();
                                self.scratch.extend_from_slice(f.data);
                                if !f.still_valid() {
                                    Outcome::Overwritten
                                } else {
                                    let fd = FrameData { width: g.width, height: g.height, format: pixel_format(&g), bayer: bayer(&g), data: &self.scratch, captured_at };
                                    Outcome::Written(a.write(&fd), bytes, t)
                                }
                            }
                        }
                    }
                    Ok(_) | Err(Next::Empty) => Outcome::Retry,
                    Err(Next::Lapped(n)) => Outcome::Lapped(before + 1, before + n),
                }
            };
            match outcome {
                Outcome::Retry => break,
                Outcome::Lapped(from, to) => {
                    self.note_lost(from, to);
                    self.sh.queued.fetch_sub((to - from + 1) as usize, Ordering::Relaxed);
                    progressed = true;
                    continue;
                }
                Outcome::Written(Ok(()), bytes, t) => {
                    self.sh.written_frames.fetch_add(1, Ordering::Relaxed);
                    self.sh.written_bytes.fetch_add(bytes, Ordering::Relaxed);
                    let inst = bytes as f64 / t.elapsed().as_secs_f64().max(1e-6);
                    let old = self.sh.rate.load(Ordering::Relaxed) as f64;
                    let ema = if old == 0.0 { inst } else { 0.8 * old + 0.2 * inst };
                    self.sh.rate.store(ema as u64, Ordering::Relaxed);
                }
                Outcome::Written(Err(e), _, _) => *self.sh.error.lock().unwrap() = Some(e),
                Outcome::Overwritten => self.note_lost(seq, seq),
                Outcome::Discarded => {}
            }
            let c = self.consumer.as_ref().unwrap();
            // give the written frame's memory back when nobody else needs it and the slot is still protected by our hold
            // (another reader that has pinned this frame, the stacker working on it, keeps it; a reader that is merely attached does not)
            let pinned = c.ring().consumers().iter().any(|i| i.pin_seq != 0 && i.pin_seq <= seq);
            if c.must_not_lose() && !pinned && !self.sh.keep.lock().unwrap().contains(&burst) {
                c.release_slot_memory(seq);
            }
            c.advance(seq);
            self.sh.queued.fetch_sub(1, Ordering::Relaxed);
            progressed = true;
        }
        (progressed, need_control)
    }

    /// Moves past all the frames of a deferred burst without writing them and reports it as skipped. False if the burst has not ended.
    fn skip_burst(&mut self, id: u64) -> bool {
        let Some(p) = self.pending.iter().find(|p| p.id == id) else { return false };
        let Some(end) = p.end else { return false };
        let spec = p.spec.clone();
        if self.active.as_ref().map(|(i, _)| *i) == Some(id) {
            self.active = None; // the working state made when the burst's first frame was seen: nothing was written, it is not reported
        }
        let c = self.consumer.as_ref().unwrap();
        let before = c.read_seq();
        // frames the camera has already overwritten (a stack-only burst that waited too long) are lost, not skipped: say so
        let oldest = c.ring().oldest_possible();
        if before + 1 < oldest {
            let to = (oldest - 1).min(end);
            self.note_lost(before + 1, to);
        }
        let c = self.consumer.as_ref().unwrap();
        // give the skipped frames' memory back, except any the camera is about to reach (a frame being written must not lose its page)
        let ring = c.ring();
        for seq in (before + 1).max(oldest)..=end {
            if ring.frames_until_overwritten(seq) >= 3 {
                ring.release_slot_memory(seq);
            }
        }
        c.advance(end);
        self.sh.queued.fetch_sub((end.saturating_sub(before)) as usize, Ordering::Relaxed);
        let mut a = Active::new(spec);
        a.skipped = true;
        a.dropped = self.take_lost(id);
        if let Some(p) = self.pending.iter_mut().find(|p| p.id == id) {
            p.finished = true;
        }
        self.complete(a);
        true
    }

    /// Completes every ended burst whose frames have all been taken.
    fn finish_ended(&mut self) -> bool {
        let mut any = false;
        loop {
            let done = self.consumer.as_ref().map_or(0, |c| c.read_seq());
            let Some(front) = self.pending.front() else { break };
            let Some(end) = front.end else { break };
            if done < end {
                break;
            }
            let p = self.pending.pop_front().unwrap();
            if !p.finished {
                match self.active.take() {
                    Some((id, mut a)) if id == p.id => {
                        a.dropped = self.take_lost(id);
                        self.complete(a)
                    }
                    other => {
                        self.active = other;
                        let mut a = Active::new(p.spec);
                        a.dropped = self.take_lost(p.id);
                        self.complete(a); // a burst with no frames, or one that lost them all: still finalised and reported
                    }
                }
            }
            any = true;
        }
        any
    }
}

impl FrameSink {
    /// `capacity_frames` is the RAM buffer size in frames (at least 2). Wait mode: `push` blocks while the buffer is full.
    pub fn new(capacity_frames: usize) -> FrameSink { FrameSink::with_policy(capacity_frames, false) }

    /// In drop mode the camera side never waits for the writer: what the writer cannot keep up with is overwritten and counted dropped.
    pub fn with_policy(capacity_frames: usize, drop_when_full: bool) -> FrameSink {
        let (tx, rx) = channel::<Msg>();
        let shared = Arc::new(Shared::default());
        let mut w = Writer { sh: shared.clone(), consumer: None, pending: VecDeque::new(), active: None, quit: false, scratch: Vec::new(), lost: Default::default(), hold: !drop_when_full };
        let handle = std::thread::Builder::new()
            .name("frame-writer".into())
            .spawn(move || loop {
                #[cfg(test)]
                while w.sh.pause.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(1));
                }
                if w.quit {
                    // shutting down with a burst still waiting for its decision: its video is written, never thrown away
                    for p in w.pending.iter_mut().filter(|p| p.deferred && p.resolution.is_none()) {
                        p.resolution = Some(Resolution::WriteAll);
                    }
                }
                loop {
                    match rx.try_recv() {
                        Ok(m) => w.on_msg(m),
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            w.quit = true;
                            break;
                        }
                    }
                }
                let (progressed, need_control) = w.drain_ring();
                let ended = w.finish_ended();
                // asked to stop and nothing is left in the ring: finish what is open (a burst whose End never came, because its owner was
                // dropped or panicked, is closed as it is) and exit, so that dropping a sink can never hang
                if w.quit && w.consumer.as_ref().map_or(true, |c| c.backlog() == 0) && !progressed && !need_control {
                    w.finish_ended();
                    w.finish_active();
                    break;
                }
                if !progressed && !ended && !need_control {
                    match rx.recv_timeout(Duration::from_millis(2)) {
                        Ok(m) => w.on_msg(m),
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => w.quit = true,
                    }
                }
            })
            .expect("spawn writer thread");
        FrameSink { tx, shared, handle: Some(handle), capacity: capacity_frames.max(2), drop_when_full, feed: Mutex::new(None), burst_id: AtomicU64::new(0) }
    }

    /// Starts a burst that is written as its frames arrive. Returns the burst's id.
    pub fn begin(&self, spec: BurstSpec) -> u64 { self.begin_burst(spec, false) }

    /// Starts a burst that is written as its frames arrive, but whose frames stay in the ring afterwards (until the camera wraps over
    /// them) because another process is going to stack them: the writer does not give their memory back. Returns the burst's id.
    pub fn begin_kept(&self, spec: BurstSpec) -> u64 {
        let id = self.begin_burst(spec, false);
        self.shared.keep.lock().unwrap().insert(id);
        id
    }

    /// Starts a burst whose frames stay in the ring until `resolve` says what to write (a stack is made from them first).
    pub fn begin_deferred(&self, spec: BurstSpec) -> u64 { self.begin_burst(spec, true) }

    fn begin_burst(&self, spec: BurstSpec, deferred: bool) -> u64 {
        let id = self.burst_id.fetch_add(1, Ordering::Relaxed) + 1;
        // the first frame this burst can have, recorded before any of its frames is committed
        let first = self.feed.lock().unwrap().as_ref().map_or(1, |f| f.ring.newest() + 1);
        self.shared.ranges.lock().unwrap().push(BurstRange { id, first, last: None });
        let _ = self.tx.send(Msg::Begin { id, spec, deferred });
        id
    }

    /// Says what to do with the raw video of a deferred burst. Until then its frames wait in the ring.
    pub fn resolve(&self, id: u64, resolution: Resolution) {
        let _ = self.tx.send(Msg::Resolve { id, resolution });
    }

    /// The ring the frames are in (made at the first frame), for consumers that read it in place, and a descriptor to hand to another
    /// process.
    pub fn ring(&self) -> Option<Ring> { self.feed.lock().unwrap().as_ref().map(|f| f.ring.clone()) }
    pub fn ring_fd(&self) -> Option<std::os::fd::OwnedFd> { self.feed.lock().unwrap().as_ref().and_then(|f| f.fd.try_clone().ok()) }

    /// Makes the ring on the first frame, and registers the writer in it before anything is committed.
    fn feed_for<'a>(&self, slot: &'a mut Option<Feed>, f: &Frame) -> Result<&'a mut Feed, String> {
        if slot.is_none() {
            let g = geometry_of(f);
            let (ring, fd) = Ring::create(g, self.capacity + 1).map_err(|e| e.to_string())?;
            let consumer = ring.attach_consumer("raw-writer", !self.drop_when_full, false).map_err(|e| e.to_string())?;
            let producer = ring.become_producer();
            self.tx.send(Msg::Ring(consumer)).map_err(|_| "the writer thread has stopped".to_string())?;
            *slot = Some(Feed { ring, fd, producer, geometry: g });
        }
        let feed = slot.as_mut().unwrap();
        if feed.geometry != geometry_of(f) {
            return Err(format!("the frame size changed during a recording ({}x{} to {}x{})", feed.geometry.width, feed.geometry.height, f.width, f.height));
        }
        Ok(feed)
    }

    fn meta(&self, f: &Frame) -> FrameMeta {
        let ts = f.captured_at.duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
        FrameMeta { ts_ns: ts, burst_id: self.burst_id.load(Ordering::Relaxed), ..Default::default() }
    }

    /// Commits a frame; blocks while the RAM buffer is full (wait mode). Returns false if the writer has died or the frame does not fit.
    pub fn push(&self, frame: Frame) -> bool {
        let mut slot = self.feed.lock().unwrap();
        let feed = match self.feed_for(&mut slot, &frame) {
            Ok(f) => f,
            Err(e) => {
                self.shared.error.lock().unwrap().get_or_insert(e);
                return false;
            }
        };
        let meta = self.meta(&frame);
        loop {
            match feed.producer.push(&frame.data, meta) {
                Ok(_) => {
                    self.shared.queued.fetch_add(1, Ordering::Relaxed);
                    return true;
                }
                Err(Full::HeldBack) => {
                    if self.handle.as_ref().map_or(true, |h| h.is_finished()) {
                        return false;
                    }
                    std::thread::sleep(Duration::from_micros(200));
                }
            }
        }
    }

    /// Commits a frame without ever blocking: when the RAM buffer is full the frame is refused and counted dropped (wait mode); in drop
    /// mode it is always accepted and the writer's lost frames are counted when it notices them.
    /// Returns `Some(true)` if accepted, `Some(false)` if refused, `None` if the writer has died.
    pub fn try_push(&self, frame: Frame) -> Option<bool> {
        let mut slot = self.feed.lock().unwrap();
        let feed = match self.feed_for(&mut slot, &frame) {
            Ok(f) => f,
            Err(e) => {
                self.shared.error.lock().unwrap().get_or_insert(e);
                return None;
            }
        };
        let meta = self.meta(&frame);
        match feed.producer.push(&frame.data, meta) {
            Ok(_) => {
                self.shared.queued.fetch_add(1, Ordering::Relaxed);
                Some(true)
            }
            Err(Full::HeldBack) => {
                self.shared.dropped.fetch_add(1, Ordering::Relaxed);
                Some(false)
            }
        }
    }

    /// Ends the current burst; its file is finalized after the queued frames are written.
    pub fn end(&self) -> (u64, u64, u64) {
        let id = self.burst_id.load(Ordering::Relaxed);
        let last_seq = self.feed.lock().unwrap().as_ref().map_or(0, |f| f.ring.newest());
        if let Some(r) = self.shared.ranges.lock().unwrap().iter_mut().find(|r| r.id == id) {
            r.last = Some(last_seq);
        }
        let first = self.shared.ranges.lock().unwrap().iter().find(|r| r.id == id).map_or(1, |r| r.first);
        let _ = self.tx.send(Msg::End { id, last_seq });
        (id, first, last_seq)
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

/// A frame as the writer sees it: its bytes read in place from the ring (no copy) and what is needed to write it.
struct FrameData<'a> {
    width: u32,
    height: u32,
    format: rustflare_core::PixelFormat,
    bayer: Option<rustflare_core::BayerPattern>,
    data: &'a [u8],
    captured_at: std::time::SystemTime,
}

impl FrameData<'_> {
    /// An owned frame, for the image-file formats (a copy; they are not the fast path).
    fn to_frame(&self) -> Frame {
        let mut f = Frame::new(self.width, self.height, self.format, self.data.to_vec());
        f.captured_at = self.captured_at;
        f.bayer = self.bayer;
        f
    }
}

struct Active {
    spec: BurstSpec,
    ser: Option<SerWriter>,
    frames: u32,
    parts: u32,
    bytes: u64,
    dropped: u64,
    skipped: bool,
    /// When the first frame of this burst reached the writer.
    started: Option<Instant>,
}

impl Active {
    fn new(spec: BurstSpec) -> Active {
        Active { spec, ser: None, frames: 0, parts: 0, bytes: 0, dropped: 0, skipped: false, started: None }
    }

    fn part_path(&self, part: u32) -> PathBuf {
        let mut name = self.spec.stem.file_name().unwrap_or_default().to_os_string();
        if part > 1 {
            name.push(format!("_part{part}"));
        }
        name.push(".ser");
        self.spec.stem.with_file_name(name)
    }

    fn write(&mut self, f: &FrameData<'_>) -> Result<(), String> {
        self.started.get_or_insert_with(Instant::now);
        self.bytes += f.data.len() as u64;
        match self.spec.format {
            Format::Png => {
                std::fs::create_dir_all(&self.spec.stem).map_err(|e| format!("{}: {e}", self.spec.stem.display()))?;
                let p = self.spec.stem.join(format!("frame_{:06}.png", self.frames + 1));
                write_png(&p, &f.to_frame()).map_err(|e| e.to_string())?;
            }
            Format::Fits => {
                std::fs::create_dir_all(&self.spec.stem).map_err(|e| format!("{}: {e}", self.spec.stem.display()))?;
                let p = self.spec.stem.join(format!("frame_{:06}.fits", self.frames + 1));
                write_fits(&p, &f.to_frame(), &self.spec.fits_meta).map_err(|e| e.to_string())?;
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
                self.ser.as_mut().unwrap().write_frame_data(f.width, f.height, f.format, f.data, f.captured_at).map_err(|e| e.to_string())?;
            }
        }
        self.frames += 1;
        Ok(())
    }

    /// Finalizes the burst and forces it onto the storage medium: "saved" must mean safe to unplug, and
    /// the returned throughput (bytes/s) is the *real* write speed, not the speed of filling the OS cache.
    fn finish(mut self) -> Result<(Finished, f64), String> {
        if self.skipped {
            let path = self.part_path(1);
            return Ok((Finished { path, frames: 0, parts: 1, dropped: self.dropped, skipped: true }, 0.0));
        }
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
        if self.frames > 0 {
            write_ready_marker(&path, self.frames, self.parts.max(1)); // a burst that lost every frame has no file to mark
        }
        Ok((Finished { path, frames: self.frames, parts: self.parts.max(1), dropped: self.dropped, skipped: false }, rate))
    }
}

/// Where the "this recording is complete" marker for `recording` is: `<name>.ready` next to the SER file or the image folder. It is
/// written last, after the file is finalized and flushed, so a watcher (the stacker) can trust it: the burst's sidecar is written
/// when the burst ends, while its frames may still be queued for the writer.
pub fn ready_marker_path(recording: &std::path::Path) -> PathBuf {
    let mut name = recording.as_os_str().to_os_string();
    name.push(".ready");
    PathBuf::from(name)
}

fn write_ready_marker(path: &std::path::Path, frames: u32, parts: u32) {
    let marker = ready_marker_path(path);
    let text = format!("{{\"frames\":{frames},\"parts\":{parts}}}\n");
    if let Err(e) = std::fs::write(&marker, text) {
        log::warn!("could not write {}: {e}", marker.display()); // not fatal: the recording itself is safe
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
        // the marker appears once the file is complete, and says what is in it
        let marker = std::fs::read_to_string(dir.join("b1.ser.ready")).unwrap();
        assert!(marker.contains("\"frames\":20") && marker.contains("\"parts\":1"), "{marker}");
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
        // one marker for the whole split recording, written when its last part is closed
        assert!(std::fs::read_to_string(dir.join("b.ser.ready")).unwrap().contains("\"parts\":3"));
        assert!(!dir.join("b_part2.ser.ready").exists());
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

    fn numbered(n: u8) -> Frame { Frame::new(4, 4, PixelFormat::Mono16, vec![n; 32]) }

    #[test]
    fn what_is_written_is_what_was_pushed_in_order_across_several_bursts() {
        let dir = tmp("bursts");
        let sink = FrameSink::new(4); // small ring: the writer is held back and released many times
        for b in 0..3u8 {
            sink.begin(BurstSpec { stem: dir.join(format!("b{b}")), format: Format::Ser, instrument: "cam".into(), max_file_bytes: None, fits_meta: Default::default() });
            for i in 0..10u8 { assert!(sink.push(numbered(b * 10 + i))); }
            sink.end();
        }
        sink.close();
        while !sink.is_finished() { std::thread::sleep(Duration::from_millis(5)); }
        assert!(sink.error().is_none(), "{:?}", sink.error());
        let fin = sink.take_finished();
        assert_eq!(fin.len(), 3);
        for b in 0..3u8 {
            let bytes = std::fs::read(dir.join(format!("b{b}.ser"))).unwrap();
            assert_eq!(bytes.len(), 178 + 10 * (32 + 8));
            for i in 0..10usize {
                let at = 178 + i * 32;
                assert!(bytes[at..at + 32].iter().all(|&x| x == b * 10 + i as u8), "burst {b} frame {i}");
            }
            assert!(dir.join(format!("b{b}.ser.ready")).exists());
        }
        assert_eq!((sink.written_frames(), sink.dropped(), sink.queued()), (30, 0, 0));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn in_drop_mode_the_camera_side_never_waits_and_every_frame_is_either_written_or_counted_dropped() {
        let dir = tmp("dropmode");
        let sink = FrameSink::with_policy(3, true);
        sink.begin(BurstSpec { stem: dir.join("b"), format: Format::Ser, instrument: "cam".into(), max_file_bytes: None, fits_meta: Default::default() });
        // every pixel of frame i is i + 1: a torn frame would show as a mix of values (frames made first, so only the pushes are timed)
        let frames: Vec<Frame> = (0..300u16).map(|i| {
            let mut data = vec![0u8; 256 * 256 * 2];
            for c in data.chunks_exact_mut(2) { c.copy_from_slice(&(i + 1).to_le_bytes()); }
            Frame::new(256, 256, PixelFormat::Mono16, data)
        }).collect();
        let t = Instant::now();
        for f in frames { assert_eq!(sink.try_push(f), Some(true), "drop mode accepts every frame"); }
        assert!(t.elapsed() < Duration::from_secs(5), "pushing never waits for the writer");
        sink.end();
        sink.close();
        while !sink.is_finished() { std::thread::sleep(Duration::from_millis(5)); }
        assert!(sink.error().is_none(), "{:?}", sink.error());
        assert_eq!(sink.written_frames() + sink.dropped(), 300, "nothing is lost without being counted");
        let fin = sink.take_finished();
        assert_eq!(fin.iter().map(|f| f.dropped).sum::<u64>(), sink.dropped(), "the burst's report says how many of its frames were lost");
        assert_eq!(fin[0].frames as u64, sink.written_frames());
        assert_eq!(sink.queued(), 0);
        // what reached the file is whole frames only, in increasing order
        let bytes = std::fs::read(dir.join("b.ser")).unwrap();
        let fb = 256 * 256 * 2;
        let n = (bytes.len() - 178) / (fb + 8);
        assert_eq!(n as u64, sink.written_frames());
        let mut last = 0u16;
        for k in 0..n {
            let f = &bytes[178 + k * fb..178 + (k + 1) * fb];
            let v = u16::from_le_bytes([f[0], f[1]]);
            assert!(f.chunks_exact(2).all(|c| u16::from_le_bytes([c[0], c[1]]) == v), "frame {k} in the file is torn");
            assert!(v > last, "frames out of order: {v} after {last}");
            last = v;
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_frame_of_another_size_in_the_middle_of_a_recording_is_refused_with_a_reason() {
        let dir = tmp("geom");
        let sink = FrameSink::new(4);
        sink.begin(BurstSpec { stem: dir.join("b"), format: Format::Ser, instrument: "cam".into(), max_file_bytes: None, fits_meta: Default::default() });
        assert!(sink.push(frame()));
        assert!(!sink.push(Frame::new(8, 8, PixelFormat::Mono16, vec![0; 128])));
        assert!(sink.error().unwrap().contains("frame size changed"), "{:?}", sink.error());
        sink.end();
        sink.close();
        while !sink.is_finished() { std::thread::sleep(Duration::from_millis(5)); }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn bursts_that_overlap_in_the_buffer_are_each_finished_once_with_their_own_frame_count() {
        let dir = tmp("overlap");
        let sink = FrameSink::new(200); // all bursts fit: the writer sees later bursts before it has finished the earlier ones
        let n = 40usize;
        for b in 0..n {
            sink.begin(BurstSpec { stem: dir.join(format!("b{b:02}")), format: Format::Ser, instrument: "cam".into(), max_file_bytes: None, fits_meta: Default::default() });
            for i in 0..3u8 { assert!(sink.push(numbered(b as u8 * 3 + i))); }
            sink.end();
        }
        sink.close();
        while !sink.is_finished() { std::thread::sleep(Duration::from_millis(5)); }
        assert!(sink.error().is_none(), "{:?}", sink.error());
        let fin = sink.take_finished();
        assert_eq!(fin.len(), n, "every burst is reported exactly once");
        assert!(fin.iter().all(|f| f.frames == 3), "{:?}", fin.iter().map(|f| f.frames).collect::<Vec<_>>());
        for b in 0..n {
            let marker = std::fs::read_to_string(dir.join(format!("b{b:02}.ser.ready"))).unwrap();
            assert!(marker.contains("\"frames\":3"), "burst {b}: {marker}");
            assert_eq!(std::fs::metadata(dir.join(format!("b{b:02}.ser"))).unwrap().len(), 178 + 3 * (32 + 8));
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_lost_range_is_charged_to_the_bursts_it_belonged_to() {
        let r = |id, first, last| BurstRange { id, first, last };
        let ranges = [r(1, 1, Some(10)), r(2, 11, Some(20)), r(3, 21, None)];
        assert_eq!(attribute(&ranges, 4, 6), vec![(1, 3)]);
        assert_eq!(attribute(&ranges, 8, 14), vec![(1, 3), (2, 4)]);
        assert_eq!(attribute(&ranges, 1, 25), vec![(1, 10), (2, 10), (3, 5)]);
        assert_eq!(attribute(&ranges, 20, 21), vec![(2, 1), (3, 1)]);
        assert_eq!(attribute(&ranges, 30, 30), vec![(3, 1)], "the open burst owns everything after its first frame");
        assert!(attribute(&[r(1, 5, Some(4))], 1, 100).is_empty(), "a burst without frames owns nothing");
    }

    #[test]
    fn losses_across_several_bursts_are_each_charged_to_the_right_burst_exactly() {
        let dir = tmp("lossattr");
        let sink = FrameSink::with_policy(10, true); // 11 slots
        sink.shared.pause.store(true, Ordering::Relaxed); // the writer is stalled while the camera keeps going
        let per = [7usize, 12, 5, 9]; // bursts of different sizes; 33 frames in all
        for (b, n) in per.iter().enumerate() {
            sink.begin(BurstSpec { stem: dir.join(format!("b{b}")), format: Format::Ser, instrument: "cam".into(), max_file_bytes: None, fits_meta: Default::default() });
            for i in 0..*n { assert_eq!(sink.try_push(numbered((b * 20 + i) as u8)), Some(true)); }
            sink.end();
        }
        sink.close();
        sink.shared.pause.store(false, Ordering::Relaxed);
        while !sink.is_finished() { std::thread::sleep(Duration::from_millis(5)); }
        assert!(sink.error().is_none(), "{:?}", sink.error());
        // the ring holds the newest 11 of the 33 frames, so frames 1 to 22 were overwritten: all 7 of burst 0, all 12 of burst 1, frames
        // 20 to 22 of burst 2 (3 of its 5) and none of burst 3
        let total: usize = per.iter().sum();
        let lost_total = total - 11;
        assert_eq!(sink.dropped(), lost_total as u64);
        let mut start = 1usize;
        let mut expected = vec![];
        for n in per { let (a, b) = (start, start + n - 1); expected.push((a..=b).filter(|&q| q <= lost_total).count() as u64); start += n; }
        let mut fin = sink.take_finished();
        fin.sort_by(|a, b| a.path.cmp(&b.path)); // bursts that wrote frames are reported before those that lost them all
        assert_eq!(fin.len(), 4, "every burst is reported, also those that lost every frame");
        let got: Vec<u64> = fin.iter().map(|f| f.dropped).collect();
        assert_eq!(got, expected, "frames lost per burst");
        assert_eq!(fin.iter().map(|f| f.frames as u64 + f.dropped).sum::<u64>(), total as u64, "written + dropped = pushed, burst by burst");
        for (f, n) in fin.iter().zip(per) { assert_eq!(f.frames as u64 + f.dropped, n as u64); }
        assert!(!dir.join("b0.ser.ready").exists() && !dir.join("b1.ser.ready").exists(), "no marker for a file that was never written");
        assert!(dir.join("b2.ser.ready").exists() && dir.join("b3.ser.ready").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    fn spec(dir: &std::path::Path, name: &str) -> BurstSpec {
        BurstSpec { stem: dir.join(name), format: Format::Ser, instrument: "cam".into(), max_file_bytes: None, fits_meta: Default::default() }
    }

    fn wait_for(sink: &FrameSink, n: usize) -> Vec<Finished> {
        let mut got = vec![];
        for _ in 0..1500 {
            got.extend(sink.take_finished());
            if got.len() >= n { break; }
            std::thread::sleep(Duration::from_millis(10));
        }
        got
    }

    #[test]
    fn a_deferred_burst_waits_in_the_ring_and_nothing_is_written_until_it_is_resolved() {
        let dir = tmp("deferred");
        let sink = FrameSink::new(32);
        let id = sink.begin_deferred(spec(&dir, "d"));
        for i in 0..10u8 { assert!(sink.push(numbered(i))); }
        sink.end();
        std::thread::sleep(Duration::from_millis(150));
        assert!(!dir.join("d.ser").exists(), "nothing is written while the decision is pending");
        assert_eq!(sink.queued(), 10, "the frames are held in the buffer");
        assert!(sink.take_finished().is_empty());
        sink.resolve(id, Resolution::WriteAll);
        let fin = wait_for(&sink, 1);
        assert_eq!((fin[0].frames, fin[0].skipped, fin[0].dropped), (10, false, 0));
        assert_eq!(std::fs::metadata(dir.join("d.ser")).unwrap().len(), 178 + 10 * 40);
        assert_eq!(sink.queued(), 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_skipped_burst_writes_no_file_and_does_not_disturb_the_next_one() {
        let dir = tmp("skipped");
        let sink = FrameSink::new(32);
        let id = sink.begin_deferred(spec(&dir, "s"));
        for i in 0..10u8 { sink.push(numbered(i)); }
        sink.end();
        sink.resolve(id, Resolution::SkipRaw);
        sink.begin(spec(&dir, "n")); // an ordinary burst right after
        for i in 0..4u8 { sink.push(numbered(100 + i)); }
        sink.end();
        let mut fin = wait_for(&sink, 2);
        fin.sort_by(|a, b| a.path.cmp(&b.path));
        // sorted by file name: the ordinary burst `n` comes before the skipped `s`; each is reported exactly once
        assert_eq!(fin.len(), 2, "{fin:?}");
        assert_eq!((fin[0].skipped, fin[0].frames, fin[1].skipped, fin[1].frames), (false, 4, true, 0), "{fin:?}");
        assert!(!dir.join("s.ser").exists() && !dir.join("s.ser.ready").exists(), "no file and no marker for a skipped burst");
        assert_eq!(std::fs::metadata(dir.join("n.ser")).unwrap().len(), 178 + 4 * 40);
        assert_eq!((sink.queued(), sink.dropped()), (0, 0));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn selected_frames_are_written_in_order_and_only_those() {
        let dir = tmp("selected");
        let sink = FrameSink::new(32);
        let id = sink.begin_deferred(spec(&dir, "x"));
        for i in 0..10u8 { sink.push(numbered(i)); }
        sink.end();
        sink.resolve(id, Resolution::WriteSelected(vec![1, 3, 4, 9]));
        let fin = wait_for(&sink, 1);
        assert_eq!((fin[0].frames, fin[0].skipped), (4, false));
        let bytes = std::fs::read(dir.join("x.ser")).unwrap();
        assert_eq!(bytes.len(), 178 + 4 * 40);
        for (k, want) in [1u8, 3, 4, 9].iter().enumerate() {
            assert!(bytes[178 + k * 32..178 + (k + 1) * 32].iter().all(|b| b == want), "frame {k} should be frame {want}");
        }
        assert_eq!(sink.queued(), 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_unresolved_burst_is_written_when_the_sink_shuts_down_never_thrown_away() {
        let dir = tmp("unresolved");
        {
            let sink = FrameSink::new(32);
            sink.begin_deferred(spec(&dir, "u"));
            for i in 0..6u8 { sink.push(numbered(i)); }
            sink.end();
            sink.close();
            while !sink.is_finished() { std::thread::sleep(Duration::from_millis(5)); }
        }
        assert_eq!(std::fs::metadata(dir.join("u.ser")).unwrap().len(), 178 + 6 * 40);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn while_a_burst_waits_for_its_decision_the_camera_is_never_held_back_and_losses_are_counted() {
        let dir = tmp("waiting");
        let sink = FrameSink::new(8); // wait mode, 9 slots: the writer normally holds the producer back
        let id = sink.begin_deferred(spec(&dir, "w"));
        let t = Instant::now();
        for i in 0..40u8 { assert!(sink.push(numbered(i)), "frame {i}"); } // far more than the ring holds
        assert!(t.elapsed() < Duration::from_secs(5), "pushing did not wait for the writer");
        sink.end();
        sink.resolve(id, Resolution::WriteAll);
        let fin = wait_for(&sink, 1);
        assert_eq!(fin[0].frames as u64 + fin[0].dropped, 40, "{fin:?}");
        assert!(fin[0].dropped > 0, "the camera wrapped over the waiting burst, and that is counted against it");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_kept_burst_is_written_and_its_frames_stay_readable_in_the_ring() {
        let dir = tmp("kept");
        let sink = FrameSink::new(32);
        let id = sink.begin_kept(spec(&dir, "k"));
        for i in 0..10u8 { sink.push(numbered(i)); }
        let (_, first, last) = sink.end();
        let fin = wait_for(&sink, 1);
        assert_eq!((fin[0].frames, fin[0].skipped), (10, false));
        assert_eq!(std::fs::metadata(dir.join("k.ser")).unwrap().len(), 178 + 10 * 40);
        // another reader (the stacker) attaches only now, after the writer is done: the frames are still there, none zeroed
        let ring = sink.ring().unwrap();
        let c = ring.attach_consumer("late", false, false).unwrap();
        for seq in first..=last {
            let f = c.frame_at(seq).expect("still in the ring");
            assert!(f.data.iter().all(|b| *b == (seq - first) as u8), "frame {seq} intact");
        }
        let _ = id;
        std::fs::remove_dir_all(&dir).ok();
    }
}
