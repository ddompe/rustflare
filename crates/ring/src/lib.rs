//! A single-producer, multi-consumer ring of frame slots in shared memory (docs/adr/0019-shared-frame-ring.md).
//!
//! One `Ring` is a block of shared memory (a `memfd` on Linux, an unlinked temporary file elsewhere) mapped by several processes.
//! The producer (the recorder) writes frames into the next slot and publishes each one by storing its sequence number last; a frame
//! exists only once committed, so a crash in the middle of a frame leaves nothing half-visible. Consumers register in the shared
//! header with a read cursor and a heartbeat, so everything needed to carry on after a process dies is in the shared memory.
//!
//! **Capture never waits for a consumer.** The producer wraps around; a consumer that is slow, stalled or dead is the one that has to
//! catch up (and loses what was overwritten). The single exception is a consumer that asks to be *must-not-lose*, which holds the
//! producer back: only the raw writer in the "wait for the disk" mode does (a slot is reused only once every such consumer is past it).
//!
//! * A *best-effort* consumer (the stacker, preview, debugging taps) never holds anything back and can be lapped; it resynchronises or
//!   discards the work it was doing.
//! * A consumer can *pin* a sequence number to read from there on in place, in any order, as often as it likes (the stacker reading a
//!   burst). For a best-effort consumer the pin is advisory: it marks what the consumer is working on (visible in `consumers()`), and
//!   `frame_at` / `FrameRef::still_valid` tell it whether a frame was overwritten. `frames_until_overwritten` says how much time a
//!   burst has left.
//!
//! All the `unsafe` is in this file and `map.rs`; the API is safe.

mod map;
pub mod ipc;

use map::Mapping;
use std::os::fd::{AsRawFd, BorrowedFd, OwnedFd};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const MAGIC: u64 = 0x474E_4952_5245_4C46; // "FLERRING" read little-endian: any fixed value will do
const VERSION: u32 = 1;
const MAX_CONSUMERS: usize = 8;
/// Control area at the start of the mapping: header, consumer table and room for the burst table (a later step).
const CONTROL_BYTES: usize = 1 << 20;
const SLOT_HEADER: usize = 64;

#[repr(C)]
struct ConsumerEntry {
    state: AtomicU32, // 0 free, 1 active
    pid: AtomicU32,
    flags: AtomicU32, // bit 0: must not lose
    _pad: u32,
    read_seq: AtomicU64, // frames up to and including this sequence number are finished with
    pin_seq: AtomicU64,  // 0, or the first sequence number this consumer still needs
    heartbeat_ns: AtomicU64,
    name: [AtomicU32; 8], // 32 bytes of name
    _tail: [u8; 56],
}

#[repr(C)]
struct Header {
    magic: u64,
    version: u32,
    header_len: u32,
    epoch: u64,
    slot_payload: u64,
    slot_stride: u64,
    n_slots: u64,
    width: u32,
    height: u32,
    bits: u32,
    format: u32,
    producer_pid: AtomicU32,
    _pad: u32,
    producer_heartbeat_ns: AtomicU64,
    /// Number of frames committed so far; the newest frame has this sequence number (sequence numbers start at 1).
    write_seq: AtomicU64,
    consumers: [ConsumerEntry; MAX_CONSUMERS],
}

#[repr(C)]
struct SlotHeader {
    /// 0 while the slot is being (re)written, else the sequence number of the frame in it. Stored last, with release ordering.
    seq: AtomicU64,
    ts_ns: AtomicU64,
    burst_id: AtomicU64,
    exposure_us: AtomicU32,
    gain: AtomicU32,
    flags: AtomicU32,
    payload_len: AtomicU32,
    _tail: [u8; 24],
}

const _: () = assert!(std::mem::size_of::<SlotHeader>() == SLOT_HEADER);
const _: () = assert!(std::mem::size_of::<ConsumerEntry>() == 128);
const _: () = assert!(std::mem::size_of::<Header>() <= 4096);

/// What the frames in a ring look like.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry {
    pub width: u32,
    pub height: u32,
    pub bits: u32,
    /// 0 mono, otherwise a colour-filter pattern id (the SER ColorID: 8 RGGB, 9 GRBG, 10 GBRG, 11 BGGR).
    pub format: u32,
}

impl Geometry {
    pub fn frame_bytes(&self) -> usize { self.width as usize * self.height as usize * if self.bits > 8 { 2 } else { 1 } }
}

#[derive(Debug)]
pub enum Error {
    Os(std::io::Error),
    /// The shared memory is not a ring of this version (or is damaged).
    BadRing(&'static str),
    /// All consumer slots are in use.
    NoConsumerSlot,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Os(e) => write!(f, "{e}"),
            Error::BadRing(w) => write!(f, "not a usable frame ring: {w}"),
            Error::NoConsumerSlot => write!(f, "no free consumer slot in the ring"),
        }
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error { fn from(e: std::io::Error) -> Self { Error::Os(e) } }

/// The system's memory page size (4 KB on most machines, 16 KB on a Raspberry Pi 5, 64 KB on some arm64 servers), never below 4 KB.
fn page_size() -> usize {
    #[cfg(unix)]
    {
        let p = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if p > 0 { return (p as usize).max(4096); }
    }
    4096
}

pub fn now_ns() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0) }

/// The shared memory and its layout. Cheap to clone (it is shared).
#[derive(Clone)]
pub struct Ring {
    map: Arc<Mapping>,
    slot_stride: usize,
    n_slots: u64,
    slot_payload: usize,
    geometry: Geometry,
    epoch: u64,
}

impl Ring {
    /// A new ring of `n_slots` frames of the given geometry. The descriptor can be handed to another process (`Ring::from_fd`).
    pub fn create(geometry: Geometry, n_slots: usize) -> Result<(Ring, OwnedFd), Error> {
        assert!(n_slots >= 2, "a ring needs at least two slots");
        let slot_payload = geometry.frame_bytes();
        let page = page_size();
        let slot_stride = (SLOT_HEADER + slot_payload + page - 1) & !(page - 1); // slots start on page boundaries (needed to release their memory)
        let total = CONTROL_BYTES + slot_stride * n_slots;
        let (fd, map) = Mapping::create(total)?;
        let epoch = now_ns();
        unsafe {
            // the mapping is zero-filled; set the immutable fields before anyone else can see the ring
            let h = map.ptr() as *mut Header;
            std::ptr::write(std::ptr::addr_of_mut!((*h).magic), MAGIC);
            std::ptr::write(std::ptr::addr_of_mut!((*h).version), VERSION);
            std::ptr::write(std::ptr::addr_of_mut!((*h).header_len), std::mem::size_of::<Header>() as u32);
            std::ptr::write(std::ptr::addr_of_mut!((*h).epoch), epoch);
            std::ptr::write(std::ptr::addr_of_mut!((*h).slot_payload), slot_payload as u64);
            std::ptr::write(std::ptr::addr_of_mut!((*h).slot_stride), slot_stride as u64);
            std::ptr::write(std::ptr::addr_of_mut!((*h).n_slots), n_slots as u64);
            std::ptr::write(std::ptr::addr_of_mut!((*h).width), geometry.width);
            std::ptr::write(std::ptr::addr_of_mut!((*h).height), geometry.height);
            std::ptr::write(std::ptr::addr_of_mut!((*h).bits), geometry.bits);
            std::ptr::write(std::ptr::addr_of_mut!((*h).format), geometry.format);
        }
        Ok((Ring { map: Arc::new(map), slot_stride, n_slots: n_slots as u64, slot_payload, geometry, epoch }, fd))
    }

    /// Maps a ring from a descriptor received from another process (or kept from an earlier run).
    pub fn from_fd(fd: BorrowedFd<'_>) -> Result<Ring, Error> {
        let map = Mapping::from_fd(fd.as_raw_fd())?;
        if map.len() < CONTROL_BYTES {
            return Err(Error::BadRing("too small"));
        }
        let ring = unsafe {
            let h = &*(map.ptr() as *const Header);
            if h.magic != MAGIC { return Err(Error::BadRing("wrong magic")); }
            if h.version != VERSION { return Err(Error::BadRing("version mismatch")); }
            let (n, stride, payload) = (h.n_slots, h.slot_stride as usize, h.slot_payload as usize);
            if n < 2 || stride < SLOT_HEADER + payload || CONTROL_BYTES + stride * n as usize > map.len() {
                return Err(Error::BadRing("geometry does not fit the memory"));
            }
            Ring { slot_stride: stride, n_slots: n, slot_payload: payload, geometry: Geometry { width: h.width, height: h.height, bits: h.bits, format: h.format }, epoch: h.epoch, map: Arc::new(map) }
        };
        Ok(ring)
    }

    pub fn geometry(&self) -> Geometry { self.geometry }
    pub fn n_slots(&self) -> usize { self.n_slots as usize }
    pub fn frame_bytes(&self) -> usize { self.slot_payload }
    /// Bytes from the start of one slot to the next (a multiple of the system page size).
    pub fn slot_stride(&self) -> usize { self.slot_stride }
    /// Identifies this ring; a different value after a restart means the shared memory was replaced.
    pub fn epoch(&self) -> u64 { self.epoch }
    /// Bytes of shared memory in use.
    pub fn memory_bytes(&self) -> usize { self.map.len() }

    fn header(&self) -> &Header { unsafe { &*(self.map.ptr() as *const Header) } }

    fn slot_ptr(&self, seq: u64) -> *mut u8 {
        let idx = ((seq - 1) % self.n_slots) as usize;
        unsafe { self.map.ptr().add(CONTROL_BYTES + idx * self.slot_stride) }
    }
    fn slot_header(&self, seq: u64) -> &SlotHeader { unsafe { &*(self.slot_ptr(seq) as *const SlotHeader) } }

    /// Sequence number of the newest committed frame (0 if none yet).
    pub fn newest(&self) -> u64 { self.header().write_seq.load(Ordering::Acquire) }

    /// How many more frames can be committed before frame `seq` is overwritten (0 if it already has been): the time a burst has left
    /// is this divided by the frame rate. Used to decide whether a burst can be stacked from memory before its first frames go.
    pub fn frames_until_overwritten(&self, seq: u64) -> u64 {
        (seq + self.n_slots).saturating_sub(self.newest() + 1)
    }

    /// The oldest sequence number that may still be in its slot.
    pub fn oldest_possible(&self) -> u64 { self.newest().saturating_sub(self.n_slots - 1).max(1) }

    /// The oldest sequence number any must-not-lose consumer still needs (the producer may not overwrite it); `u64::MAX` if none.
    fn retain_from(&self) -> u64 {
        let mut r = u64::MAX;
        for c in &self.header().consumers {
            if c.state.load(Ordering::Acquire) == 1 && c.flags.load(Ordering::Relaxed) & 1 == 1 {
                let mut need = c.read_seq.load(Ordering::Acquire) + 1;
                let pin = c.pin_seq.load(Ordering::Acquire);
                if pin != 0 { need = need.min(pin); }
                r = r.min(need);
            }
        }
        r
    }

    /// Names, pids and cursors of the registered consumers (for status and telemetry).
    pub fn consumers(&self) -> Vec<ConsumerInfo> {
        self.header().consumers.iter().enumerate().filter(|(_, c)| c.state.load(Ordering::Acquire) == 1).map(|(i, c)| {
            let mut name = Vec::new();
            for w in &c.name { name.extend_from_slice(&w.load(Ordering::Relaxed).to_le_bytes()); }
            let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
            ConsumerInfo { index: i, name: String::from_utf8_lossy(&name[..end]).to_string(), pid: c.pid.load(Ordering::Relaxed), must_not_lose: c.flags.load(Ordering::Relaxed) & 1 == 1, read_seq: c.read_seq.load(Ordering::Acquire), pin_seq: c.pin_seq.load(Ordering::Acquire), heartbeat_ns: c.heartbeat_ns.load(Ordering::Relaxed) }
        }).collect()
    }

    /// Frees the registrations of consumers whose heartbeat is older than `timeout_ns` (or whose process no longer exists), releasing
    /// whatever they held back. Returns how many were removed. The producer calls this when it finds itself blocked.
    pub fn reap_dead(&self, timeout_ns: u64) -> usize {
        let now = now_ns();
        let mut n = 0;
        for c in &self.header().consumers {
            if c.state.load(Ordering::Acquire) != 1 { continue; }
            let stale = now.saturating_sub(c.heartbeat_ns.load(Ordering::Relaxed)) > timeout_ns;
            let pid = c.pid.load(Ordering::Relaxed);
            let gone = pid != 0 && !process_exists(pid);
            if stale || gone {
                c.pin_seq.store(0, Ordering::Release);
                c.state.store(0, Ordering::Release);
                n += 1;
            }
        }
        n
    }

    /// Registers a consumer. It starts at the newest frame (frames committed from now on), unless `from_oldest` is set.
    pub fn attach_consumer(&self, name: &str, must_not_lose: bool, from_oldest: bool) -> Result<Consumer, Error> {
        let start = if from_oldest { self.oldest_possible() - 1 } else { self.newest() };
        for (i, c) in self.header().consumers.iter().enumerate() {
            if c.state.compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                // claimed (state 2: being set up); fill in, then publish as active
                c.read_seq.store(start, Ordering::Release);
                c.pin_seq.store(0, Ordering::Release);
                c.flags.store(must_not_lose as u32, Ordering::Release);
                c.pid.store(std::process::id(), Ordering::Release);
                c.heartbeat_ns.store(now_ns(), Ordering::Release);
                let mut bytes = [0u8; 32];
                let b = name.as_bytes();
                bytes[..b.len().min(31)].copy_from_slice(&b[..b.len().min(31)]);
                for (k, w) in c.name.iter().enumerate() { w.store(u32::from_le_bytes([bytes[4 * k], bytes[4 * k + 1], bytes[4 * k + 2], bytes[4 * k + 3]]), Ordering::Relaxed); }
                c.state.store(1, Ordering::Release);
                return Ok(Consumer { ring: self.clone(), index: i });
            }
        }
        Err(Error::NoConsumerSlot)
    }

    /// Takes over as the producer, for the first time or after the previous producer died. Frames already committed stay valid and
    /// numbering continues; a slot that was being written when the old producer died was never committed and is simply reused.
    pub fn become_producer(&self) -> Producer {
        let h = self.header();
        h.producer_pid.store(std::process::id(), Ordering::Release);
        h.producer_heartbeat_ns.store(now_ns(), Ordering::Release);
        Producer { ring: self.clone() }
    }

    /// Whether the producer process is alive (heartbeat recent and the pid exists).
    pub fn producer_alive(&self, timeout_ns: u64) -> bool {
        let h = self.header();
        let pid = h.producer_pid.load(Ordering::Acquire);
        pid != 0 && process_exists(pid) && now_ns().saturating_sub(h.producer_heartbeat_ns.load(Ordering::Acquire)) <= timeout_ns
    }

    /// Gives the memory behind frame `seq`'s slot back to the system (Linux: `MADV_REMOVE` on the shared memory). Without this a ring
    /// that cycles through all its slots keeps all its memory resident even when only one burst is in flight. After this the frame
    /// reads as gone. Only safe while nothing can be writing to or reading the slot: call it for a frame a must-not-lose consumer has
    /// finished with and has not yet advanced past (the producer cannot reuse the slot until it does).
    pub fn release_slot_memory(&self, seq: u64) {
        #[cfg(target_os = "linux")]
        {
            // whole pages only: a slot that does not start and end on a page boundary would take its neighbour's memory with it (the
            // Raspberry Pi 5's kernel has 16 KB pages, not 4 KB)
            let page = page_size();
            let p = self.slot_ptr(seq);
            if self.slot_stride % page != 0 || (p as usize) % page != 0 {
                return;
            }
            unsafe {
                libc::madvise(p as *mut libc::c_void, self.slot_stride, libc::MADV_REMOVE);
            }
        }
        #[cfg(not(target_os = "linux"))]
        let _ = seq;
    }

    /// Reads frame `seq` in place if it is still in the ring. `None` if it is not committed yet or has been overwritten.
    fn frame(&self, seq: u64) -> Option<FrameRef<'_>> {
        if seq == 0 || seq > self.newest() { return None; }
        let sh = self.slot_header(seq);
        if sh.seq.load(Ordering::Acquire) != seq { return None; }
        let len = (sh.payload_len.load(Ordering::Relaxed) as usize).min(self.slot_payload);
        let data = unsafe { std::slice::from_raw_parts(self.slot_ptr(seq).add(SLOT_HEADER), len) };
        Some(FrameRef { ring: self, seq, data, ts_ns: sh.ts_ns.load(Ordering::Relaxed), burst_id: sh.burst_id.load(Ordering::Relaxed), exposure_us: sh.exposure_us.load(Ordering::Relaxed), gain: sh.gain.load(Ordering::Relaxed), flags: sh.flags.load(Ordering::Relaxed) })
    }
}

fn process_exists(pid: u32) -> bool {
    #[cfg(unix)]
    unsafe {
        libc::kill(pid as i32, 0) == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(not(unix))]
    { let _ = pid; true }
}

#[derive(Clone, Debug)]
pub struct ConsumerInfo {
    pub index: usize,
    pub name: String,
    pub pid: u32,
    pub must_not_lose: bool,
    pub read_seq: u64,
    pub pin_seq: u64,
    pub heartbeat_ns: u64,
}

/// A frame read in place from the ring. For a must-not-lose consumer, or one that has pinned the frame, it cannot be overwritten while
/// held; a best-effort consumer should call `still_valid` after using the data.
pub struct FrameRef<'a> {
    ring: &'a Ring,
    pub seq: u64,
    pub data: &'a [u8],
    pub ts_ns: u64,
    pub burst_id: u64,
    pub exposure_us: u32,
    pub gain: u32,
    pub flags: u32,
}

impl FrameRef<'_> {
    /// True if the slot still holds this frame (it has not been overwritten while it was being read).
    pub fn still_valid(&self) -> bool { self.ring.slot_header(self.seq).seq.load(Ordering::Acquire) == self.seq }
}

/// What the producer sets on a frame besides its bytes.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameMeta {
    pub ts_ns: u64,
    pub burst_id: u64,
    pub exposure_us: u32,
    pub gain: u32,
    pub flags: u32,
}

pub struct Producer {
    ring: Ring,
}

/// Why `begin` could not give a slot.
#[derive(Debug, PartialEq, Eq)]
pub enum Full {
    /// A must-not-lose consumer (or a pin) still needs the slot that would be reused.
    HeldBack,
}

impl Producer {
    pub fn ring(&self) -> &Ring { &self.ring }

    pub fn heartbeat(&self) { self.ring.header().producer_heartbeat_ns.store(now_ns(), Ordering::Release); }

    /// Reserves the next slot for writing. The frame becomes visible only when `commit` is called.
    pub fn begin(&mut self) -> Result<SlotWriter<'_>, Full> {
        let seq = self.ring.newest() + 1;
        if seq > self.ring.n_slots && seq - self.ring.n_slots >= self.ring.retain_from() {
            return Err(Full::HeldBack);
        }
        let sh = self.ring.slot_header(seq);
        sh.seq.store(0, Ordering::Release); // the slot's old frame is gone from now on; lapped readers notice
        std::sync::atomic::fence(Ordering::SeqCst);
        let data = unsafe { std::slice::from_raw_parts_mut(self.ring.slot_ptr(seq).add(SLOT_HEADER), self.ring.slot_payload) };
        Ok(SlotWriter { ring: &self.ring, seq, data, meta: FrameMeta::default(), len: 0 })
    }

    /// Convenience: copy `bytes` into the next slot and commit. Returns the sequence number.
    pub fn push(&mut self, bytes: &[u8], meta: FrameMeta) -> Result<u64, Full> {
        let mut w = self.begin()?;
        let n = bytes.len().min(w.data.len());
        w.data[..n].copy_from_slice(&bytes[..n]);
        w.set_len(n);
        w.set_meta(meta);
        Ok(w.commit())
    }
}

pub struct SlotWriter<'a> {
    ring: &'a Ring,
    seq: u64,
    /// The slot's payload area (the full frame size); write the frame here.
    pub data: &'a mut [u8],
    meta: FrameMeta,
    len: usize,
}

impl SlotWriter<'_> {
    pub fn set_len(&mut self, n: usize) { self.len = n; }
    pub fn set_meta(&mut self, m: FrameMeta) { self.meta = m; }
    pub fn seq(&self) -> u64 { self.seq }

    /// Publishes the frame: slot fields first, then the slot's sequence number, then the ring's write counter (all release).
    pub fn commit(self) -> u64 {
        let sh = self.ring.slot_header(self.seq);
        sh.ts_ns.store(self.meta.ts_ns, Ordering::Relaxed);
        sh.burst_id.store(self.meta.burst_id, Ordering::Relaxed);
        sh.exposure_us.store(self.meta.exposure_us, Ordering::Relaxed);
        sh.gain.store(self.meta.gain, Ordering::Relaxed);
        sh.flags.store(self.meta.flags, Ordering::Relaxed);
        sh.payload_len.store(if self.len == 0 { self.data.len() as u32 } else { self.len as u32 }, Ordering::Relaxed);
        sh.seq.store(self.seq, Ordering::Release);
        self.ring.header().write_seq.store(self.seq, Ordering::Release);
        self.seq
    }
}

/// What happened when a consumer asked for its next frame.
#[derive(Debug, PartialEq, Eq)]
pub enum Next {
    /// Nothing newer than what the consumer has already finished with.
    Empty,
    /// The consumer was lapped: this many frames were overwritten before it read them; it has been moved to the oldest frame left.
    Lapped(u64),
}

pub struct Consumer {
    ring: Ring,
    index: usize,
}

impl Consumer {
    fn entry(&self) -> &ConsumerEntry { &self.ring.header().consumers[self.index] }
    pub fn ring(&self) -> &Ring { &self.ring }
    pub fn must_not_lose(&self) -> bool { self.entry().flags.load(Ordering::Relaxed) & 1 == 1 }
    /// Switches whether this consumer holds the producer back. A writer that is waiting for a decision about a burst lets go (so the
    /// camera is never held for it) and takes the hold again when it starts writing.
    pub fn set_must_not_lose(&self, hold: bool) { self.entry().flags.store(hold as u32, Ordering::Release); }
    pub fn heartbeat(&self) { self.entry().heartbeat_ns.store(now_ns(), Ordering::Release); }
    /// The sequence number up to which this consumer is finished.
    pub fn read_seq(&self) -> u64 { self.entry().read_seq.load(Ordering::Acquire) }
    /// Frames committed that this consumer has not finished with yet.
    pub fn backlog(&self) -> u64 { self.ring.newest().saturating_sub(self.read_seq()) }

    /// The next frame after the cursor, in place; the cursor does not move until `advance`.
    pub fn peek(&mut self) -> Result<FrameRef<'_>, Next> {
        let want = self.read_seq() + 1;
        if want > self.ring.newest() { return Err(Next::Empty); }
        let oldest = self.ring.oldest_possible();
        if want < oldest {
            // lapped (only a best-effort consumer can be): skip to the oldest frame still there
            let lost = oldest - want;
            self.entry().read_seq.store(oldest - 1, Ordering::Release);
            return Err(Next::Lapped(lost));
        }
        match self.ring.frame(want) {
            Some(f) => Ok(f),
            None => Err(Next::Empty), // being rewritten right now, or not visible yet: try again
        }
    }

    /// Moves the cursor past `seq` (frames up to and including `seq` are finished with).
    pub fn advance(&self, seq: u64) {
        let c = self.entry();
        if seq > c.read_seq.load(Ordering::Relaxed) { c.read_seq.store(seq, Ordering::Release); }
    }

    /// Marks `from_seq` on as the range this consumer is working on until `unpin`, whatever the cursor says. For a must-not-lose consumer
    /// the producer will not reuse those slots; for a best-effort one it is advisory (see the module notes). Returns false if `from_seq`
    /// has already been overwritten.
    pub fn pin(&self, from_seq: u64) -> bool {
        let c = self.entry();
        c.pin_seq.store(from_seq, Ordering::SeqCst);
        std::sync::atomic::fence(Ordering::SeqCst);
        if from_seq < self.ring.oldest_possible() {
            c.pin_seq.store(0, Ordering::Release);
            return false;
        }
        true
    }

    pub fn unpin(&self) { self.entry().pin_seq.store(0, Ordering::Release); }

    /// See `Ring::release_slot_memory`: only for a must-not-lose consumer, before it advances past `seq`, and only if no other consumer
    /// needs the frame.
    pub fn release_slot_memory(&self, seq: u64) { self.ring.release_slot_memory(seq) }

    /// Frame `seq` in place, for random access while pinned.
    pub fn frame_at(&self, seq: u64) -> Option<FrameRef<'_>> { self.ring.frame(seq) }
}

impl Drop for Consumer {
    fn drop(&mut self) {
        let c = self.entry();
        c.pin_seq.store(0, Ordering::Release);
        c.state.store(0, Ordering::Release);
    }
}

#[cfg(test)]
mod tests;
