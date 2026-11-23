//! Reading SER video files (the recorder's output), including files split for FAT32 (`name.ser`, `name_part2.ser`, ...).
//!
//! Pixel data is taken as little-endian whatever the header's endianness flag says: RustFlare, SharpCap, FireCapture and
//! most cameras write it so, and the flag is unreliable in practice (see `rustflare-capture`'s writer).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const HEADER_LEN: u64 = 178;

#[derive(Debug)]
pub struct Part {
    path: PathBuf,
    file: File,
    /// Frames this part holds.
    pub frames: usize,
    /// Frames its header declares (0 while a recording is still running and nothing has been checkpointed).
    declared: usize,
}

/// One recording: the first file and any `_partN` files that continue it.
#[derive(Debug)]
pub struct Ser {
    parts: Vec<Part>,
    pub width: usize,
    pub height: usize,
    pub depth: u32,
    pub color_id: u32,
    pub instrument: String,
    frame_bytes: u64,
    stamp_offset: Vec<u64>,
}

/// Why a file could not be read.
#[derive(Debug)]
pub struct SerError(pub String);

impl std::fmt::Display for SerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(&self.0) }
}

impl std::error::Error for SerError {}

fn u32_at(h: &[u8], o: usize) -> u32 { u32::from_le_bytes([h[o], h[o + 1], h[o + 2], h[o + 3]]) }

fn read_header(path: &Path) -> Result<(File, [u8; 178]), SerError> {
    let mut f = File::open(path).map_err(|e| SerError(format!("{}: {e}", path.display())))?;
    let mut h = [0u8; 178];
    f.read_exact(&mut h).map_err(|e| SerError(format!("{}: header: {e}", path.display())))?;
    if &h[..14] != b"LUCAM-RECORDER" {
        return Err(SerError(format!("{}: not a SER file", path.display())));
    }
    Ok((f, h))
}

/// `name.ser`, `name_part2.ser`, `name_part3.ser`... in order, as far as they exist.
pub fn part_paths(first: &Path) -> Vec<PathBuf> {
    let mut v = vec![first.to_path_buf()];
    let stem = first.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let stem = stem.split("_part").next().unwrap_or(&stem).to_string();
    for n in 2.. {
        let p = first.with_file_name(format!("{stem}_part{n}.ser"));
        if p.exists() { v.push(p) } else { break }
    }
    v
}

impl Ser {
    pub fn open(path: &Path) -> Result<Ser, SerError> {
        let mut parts = Vec::new();
        let mut geom: Option<(usize, usize, u32, u32, String)> = None;
        for p in part_paths(path) {
            let (file, h) = read_header(&p)?;
            let (color, w, ht, depth, declared) = (u32_at(&h, 18), u32_at(&h, 26) as usize, u32_at(&h, 30) as usize, u32_at(&h, 34), u32_at(&h, 38) as usize);
            let g = (w, ht, depth, color, String::from_utf8_lossy(&h[82..122]).trim_matches(|c: char| c == '\0' || c == ' ').to_string());
            match &geom {
                None => geom = Some(g),
                Some(first) if (first.0, first.1, first.2) != (g.0, g.1, g.2) => return Err(SerError(format!("{}: size differs from the first part", p.display()))),
                _ => {}
            }
            let (w, ht) = (geom.as_ref().unwrap().0, geom.as_ref().unwrap().1);
            let planes = if color >= 100 { 3 } else { 1 };
            let fb = (w * ht * (depth as usize / 8) * planes) as u64;
            // a file cut short declares fewer frames than it holds, or more than it holds: trust the smaller
            let len = file.metadata().map_err(|e| SerError(e.to_string()))?.len();
            let by_size = (len.saturating_sub(HEADER_LEN) / fb) as usize;
            let frames = if declared == 0 { by_size } else { declared.min(by_size) };
            parts.push(Part { path: p, file, frames, declared });
        }
        let (width, height, depth, color_id, instrument) = geom.ok_or_else(|| SerError("no file".into()))?;
        if depth != 8 && depth != 16 {
            return Err(SerError(format!("{}: {depth}-bit data is not supported", path.display())));
        }
        if color_id >= 100 {
            return Err(SerError("RGB SER files are not supported".into()));
        }
        let frame_bytes = (width * height * (depth as usize / 8)) as u64;
        let mut stamp_offset = Vec::new();
        for p in &parts {
            stamp_offset.push(HEADER_LEN + p.frames as u64 * frame_bytes);
        }
        Ok(Ser { parts, width, height, depth, color_id, instrument, frame_bytes, stamp_offset })
    }

    /// Every part holds all the frames its header declares and the timestamp trailer after them: the writer has finished the file.
    pub fn is_complete(&self) -> bool {
        self.parts.iter().all(|p| p.declared > 0 && p.declared == p.frames && p.file.metadata().map(|m| m.len() >= HEADER_LEN + p.declared as u64 * (self.frame_bytes + 8)).unwrap_or(false))
    }

    pub fn frames(&self) -> usize { self.parts.iter().map(|p| p.frames).sum() }

    /// Bayer pattern id of the header's ColorID (8 to 11), if the data is a mosaic.
    pub fn is_mosaic(&self) -> bool { (8..=11).contains(&self.color_id) }

    /// Frame `i` as 16-bit values (8-bit data is scaled up by 257 so both depths share a range).
    pub fn read_frame(&mut self, i: usize) -> Result<Vec<u16>, SerError> {
        let mut rem = i;
        for p in &mut self.parts {
            if rem < p.frames {
                let mut raw = vec![0u8; self.frame_bytes as usize];
                p.file.seek(SeekFrom::Start(HEADER_LEN + rem as u64 * self.frame_bytes)).map_err(|e| SerError(e.to_string()))?;
                p.file.read_exact(&mut raw).map_err(|e| SerError(format!("{}: frame {i}: {e}", p.path.display())))?;
                return Ok(if self.depth == 16 { raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect() } else { raw.iter().map(|&b| b as u16 * 257).collect() });
            }
            rem -= p.frames;
        }
        Err(SerError(format!("frame {i} is past the end ({} frames)", self.frames())))
    }

    /// The capture time of frame `i` in seconds since the Unix epoch, from the timestamp trailer when the file has one.
    pub fn timestamp(&mut self, i: usize) -> Option<f64> {
        let mut rem = i;
        for (k, p) in self.parts.iter_mut().enumerate() {
            if rem < p.frames {
                let off = self.stamp_offset[k] + rem as u64 * 8;
                if p.file.metadata().ok()?.len() < off + 8 * (p.frames - rem) as u64 {
                    return None;
                }
                let mut b = [0u8; 8];
                p.file.seek(SeekFrom::Start(off)).ok()?;
                p.file.read_exact(&mut b).ok()?;
                let ticks = u64::from_le_bytes(b);
                const EPOCH_TICKS: u64 = 621_355_968_000_000_000;
                return ticks.checked_sub(EPOCH_TICKS).map(|t| t as f64 / 1e7);
            }
            rem -= p.frames;
        }
        None
    }
}

/// Tells the kernel to forget the cached pages of a recording (all its parts): the stacker reads each recording a few times and never
/// again, and its page cache is charged to its memory limit and crowds out the recorder's RAM buffer. Clean pages only, so nothing is lost.
pub fn drop_page_cache(first: &Path) {
    #[cfg(target_os = "linux")]
    for p in part_paths(first) {
        if let Ok(f) = File::open(&p) {
            use std::os::fd::AsRawFd;
            unsafe {
                libc::posix_fadvise(f.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED);
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = first;
}
