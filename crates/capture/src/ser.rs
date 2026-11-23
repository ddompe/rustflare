//! SER video writer (the de-facto format for planetary/solar capture).
//!
//! Layout follows Vidoxide's `output/ser.rs` (MIT) — including its note that the
//! header's endianness flag is 0 for little-endian pixel data in real-world
//! files, contrary to the published spec — plus the per-frame timestamp trailer
//! that Vidoxide left as a TODO.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
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

pub struct SerWriter {
    w: BufWriter<File>,
    format: (u32, u32, PixelFormat),
    stamps: Vec<u64>,
}

impl SerWriter {
    pub fn create(path: &Path, width: u32, height: u32, format: PixelFormat, instrument: &str, telescope: &str) -> Result<SerWriter> {
        Self::create_color(path, width, height, format, None, instrument, telescope)
    }

    /// Like `create`, for a raw Bayer mosaic: the header carries the pattern so stacking software demosaics it.
    pub fn create_color(path: &Path, width: u32, height: u32, format: PixelFormat, bayer: Option<rustflare_core::BayerPattern>, instrument: &str, telescope: &str) -> Result<SerWriter> {
        let mut w = BufWriter::new(File::create(path)?);
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
        w.write_all(&h)?;
        Ok(SerWriter { w, format: (width, height, format), stamps: Vec::new() })
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
        self.w.write_all(data)?;
        self.stamps.push(ticks(captured_at));
        if self.stamps.len() % CHECKPOINT_FRAMES == 0 {
            self.checkpoint()?;
        }
        Ok(())
    }

    /// Writes the current frame count into the header and returns to the end of the data.
    fn checkpoint(&mut self) -> Result<()> {
        let (w, h, f) = self.format;
        let end = HEADER_LEN as u64 + self.stamps.len() as u64 * w as u64 * h as u64 * f.bytes_per_pixel() as u64;
        self.w.flush()?;
        self.w.seek(SeekFrom::Start(FRAME_COUNT_OFFSET))?;
        self.w.write_all(&(self.stamps.len() as u32).to_le_bytes())?;
        self.w.flush()?;
        self.w.seek(SeekFrom::Start(end))?;
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
        for s in &self.stamps {
            self.w.write_all(&s.to_le_bytes())?;
        }
        self.w.seek(SeekFrom::Start(FRAME_COUNT_OFFSET))?;
        self.w.write_all(&(self.stamps.len() as u32).to_le_bytes())?;
        self.w.flush()?;
        Ok(self.stamps.len() as u32)
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
        w.w.flush().unwrap();
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
}
