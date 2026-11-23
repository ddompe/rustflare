//! Where the stacker's frames come from: a SER file, frames in memory, or a burst in the shared frame ring (ADR 0019). The stacker
//! reads frames through this trait only, so a stack made from the ring is the same computation as one made from the file.

use std::borrow::Cow;

/// A recording of 16-bit (or 8-bit, scaled up to the same range) mono frames. Shared between threads: `read_frame` takes `&self`.
pub trait FrameSource: Sync {
    fn width(&self) -> usize;
    fn height(&self) -> usize;
    fn frames(&self) -> usize;
    /// A colour-filter mosaic (not supported yet).
    fn is_mosaic(&self) -> bool { false }
    /// Capture time of the first frame, seconds since the Unix epoch, if known.
    fn first_timestamp(&self) -> Option<f64> { None }
    /// Frame `i` as 16-bit values. A source whose memory can be overwritten (the ring) returns an error rather than a torn frame.
    fn read_frame(&self, i: usize) -> Result<Cow<'_, [u16]>, String>;
    /// What this source is, for messages ("burst.ser", "ring frames 120 to 219").
    fn describe(&self) -> String;
    /// Called when the stacker is done with the source (a SER file drops its cached pages).
    fn release(&self) {}
}

/// Frames held in memory (tests, and callers that already have them).
pub struct MemFrames {
    pub width: usize,
    pub height: usize,
    pub frames: Vec<Vec<u16>>,
    pub first_timestamp: Option<f64>,
}

impl FrameSource for MemFrames {
    fn width(&self) -> usize { self.width }
    fn height(&self) -> usize { self.height }
    fn frames(&self) -> usize { self.frames.len() }
    fn first_timestamp(&self) -> Option<f64> { self.first_timestamp }
    fn read_frame(&self, i: usize) -> Result<Cow<'_, [u16]>, String> {
        self.frames.get(i).map(|f| Cow::Borrowed(&f[..])).ok_or_else(|| format!("frame {i} is past the end ({} frames)", self.frames.len()))
    }
    fn describe(&self) -> String { format!("{} frames in memory", self.frames.len()) }
}

/// A burst in the shared frame ring: `count` consecutive frames from sequence number `first_seq`. The stacker is a best-effort consumer
/// of the ring, so the camera never waits for it; a frame that has been overwritten (or is overwritten while it is being read) is
/// reported as an error and the stack job is abandoned.
pub struct RingFrames {
    consumer: rustflare_ring::Consumer,
    first_seq: u64,
    count: usize,
}

impl RingFrames {
    /// Marks the burst as being worked on (an advisory pin, visible to status and telemetry). Fails if its first frame is already gone.
    pub fn new(consumer: rustflare_ring::Consumer, first_seq: u64, count: usize) -> Result<RingFrames, String> {
        if !consumer.pin(first_seq) {
            return Err(format!("the burst's first frame (sequence {first_seq}) has already been overwritten"));
        }
        Ok(RingFrames { consumer, first_seq, count })
    }

    /// Whether every frame of the burst is still in the ring.
    pub fn intact(&self) -> bool { (0..self.count).all(|i| self.consumer.frame_at(self.first_seq + i as u64).is_some()) }
}

impl Drop for RingFrames {
    fn drop(&mut self) { self.consumer.unpin(); }
}

impl FrameSource for RingFrames {
    fn width(&self) -> usize { self.consumer.ring().geometry().width as usize }
    fn height(&self) -> usize { self.consumer.ring().geometry().height as usize }
    fn frames(&self) -> usize { self.count }
    fn is_mosaic(&self) -> bool { (8..=11).contains(&self.consumer.ring().geometry().format) }
    fn first_timestamp(&self) -> Option<f64> { self.consumer.frame_at(self.first_seq).map(|f| f.ts_ns as f64 / 1e9) }
    fn read_frame(&self, i: usize) -> Result<Cow<'_, [u16]>, String> {
        if i >= self.count {
            return Err(format!("frame {i} is past the end ({} frames)", self.count));
        }
        let seq = self.first_seq + i as u64;
        let f = self.consumer.frame_at(seq).ok_or_else(|| format!("frame {i} (sequence {seq}) was overwritten in the ring before it could be read"))?;
        // copied out and then checked: a frame the camera started to overwrite while we read it is never used
        let v: Vec<u16> = if self.consumer.ring().geometry().bits > 8 {
            f.data.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
        } else {
            f.data.iter().map(|&b| b as u16 * 257).collect()
        };
        if !f.still_valid() {
            return Err(format!("frame {i} (sequence {seq}) was overwritten in the ring while it was being read"));
        }
        Ok(Cow::Owned(v))
    }
    fn describe(&self) -> String { format!("ring frames {} to {}", self.first_seq, self.first_seq + self.count as u64 - 1) }
}
