//! Capture file review: SER header/timing/brightness statistics (to flag recordings that are mostly dark) and safe
//! deletion inside the storage folder.

use serde::Serialize;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};

const HEADER: u64 = 178;
/// A frame whose mean is below this fraction of full scale counts as dark.
const DARK_MEAN: f64 = 0.03;
/// A recording with at least this fraction of dark sampled frames is flagged.
const DARK_FRACTION: f64 = 0.7;

#[derive(Debug, Serialize, PartialEq)]
pub struct SerStats {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub frames: u32,
    /// Frames actually present in the file (a cut-off recording has fewer than the header says).
    pub frames_present: u64,
    pub complete: bool,
    pub fps: Option<f64>,
    pub duration_s: Option<f64>,
    /// Mean brightness (0..1 of full scale) of the sampled frames.
    pub mean_min: f64,
    pub mean_avg: f64,
    pub mean_max: f64,
    pub sampled: u32,
    pub dark_fraction: f64,
    /// Mostly dark: probably the cap was on, the Sun was out of view, or the exposure was far too short.
    pub mostly_dark: bool,
}

fn rd32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

/// Reads the SER header and samples up to `samples` evenly spaced frames (every 16th pixel) for brightness.
pub fn ser_stats(path: &Path, samples: u32) -> Result<SerStats, String> {
    let mut f = File::open(path).map_err(|e| e.to_string())?;
    let len = f.metadata().map_err(|e| e.to_string())?.len();
    let mut h = [0u8; HEADER as usize];
    f.read_exact(&mut h).map_err(|_| "not a SER file (too short)".to_string())?;
    if &h[..14] != b"LUCAM-RECORDER" {
        return Err("not a SER file".into());
    }
    let (w, ht, depth, frames) = (rd32(&h, 26), rd32(&h, 30), rd32(&h, 34), rd32(&h, 38));
    if w == 0 || ht == 0 || !(8..=16).contains(&depth) {
        return Err("unsupported SER header".into());
    }
    let bytes_pp = if depth > 8 { 2 } else { 1 };
    let frame_bytes = w as u64 * ht as u64 * bytes_pp;
    let present = (len.saturating_sub(HEADER) / frame_bytes).min(frames as u64);
    let complete = present == frames as u64;
    // timestamps live in a trailer after the frames: 8 bytes per frame, 100 ns ticks
    let ts_at = HEADER + frames as u64 * frame_bytes;
    let (fps, duration_s) = if complete && len >= ts_at + 8 * frames as u64 && frames > 1 {
        let mut t = [0u8; 8];
        let mut get = |i: u64| -> Option<u64> {
            f.seek(SeekFrom::Start(ts_at + 8 * i)).ok()?;
            f.read_exact(&mut t).ok()?;
            Some(u64::from_le_bytes(t))
        };
        match (get(0), get(frames as u64 - 1)) {
            (Some(a), Some(b)) if b > a => {
                let secs = (b - a) as f64 / 1e7;
                (Some((frames as f64 - 1.0) / secs), Some(secs))
            }
            _ => (None, None),
        }
    } else {
        (None, None)
    };
    let full = if depth > 8 { 65535.0 } else { 255.0 };
    let n = (samples.max(1) as u64).min(present.max(1)) as u32;
    let mut means = vec![];
    let mut buf = vec![0u8; frame_bytes as usize];
    for k in 0..n {
        if present == 0 {
            break;
        }
        let idx = if n == 1 { 0 } else { k as u64 * (present - 1) / (n as u64 - 1) };
        f.seek(SeekFrom::Start(HEADER + idx * frame_bytes)).map_err(|e| e.to_string())?;
        if f.read_exact(&mut buf).is_err() {
            break;
        }
        let (mut sum, mut cnt) = (0f64, 0u64);
        if bytes_pp == 2 {
            for px in buf.chunks_exact(2).step_by(16) {
                sum += u16::from_le_bytes([px[0], px[1]]) as f64;
                cnt += 1;
            }
        } else {
            for &px in buf.iter().step_by(16) {
                sum += px as f64;
                cnt += 1;
            }
        }
        // 12-bit cameras in 16-bit containers fill only part of the range: scale by the real depth
        let scale = if depth > 8 { ((1u32 << depth) - 1) as f64 } else { full };
        means.push(if cnt > 0 { sum / cnt as f64 / scale } else { 0.0 });
    }
    let sampled = means.len() as u32;
    let dark = means.iter().filter(|m| **m < DARK_MEAN).count() as f64;
    let dark_fraction = if sampled > 0 { dark / sampled as f64 } else { 1.0 };
    Ok(SerStats {
        width: w,
        height: ht,
        depth,
        frames,
        frames_present: present,
        complete,
        fps,
        duration_s,
        mean_min: means.iter().cloned().fold(f64::INFINITY, f64::min).min(1.0),
        mean_avg: if sampled > 0 { means.iter().sum::<f64>() / sampled as f64 } else { 0.0 },
        mean_max: means.iter().cloned().fold(0.0, f64::max),
        sampled,
        dark_fraction,
        mostly_dark: sampled > 0 && dark_fraction >= DARK_FRACTION,
    })
}

/// Resolves a path relative to `root` (e.g. `2026-11-21/seq_x.ser`) without letting it escape.
pub fn resolve(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let p = Path::new(rel);
    if p.is_absolute() || p.components().any(|c| !matches!(c, Component::Normal(_))) || rel.is_empty() {
        return Err("invalid path".into());
    }
    Ok(root.join(p))
}

/// Deletes one capture (file or PNG burst folder) and its `.json` sidecar. Exact names only, never globs.
pub fn delete(root: &Path, rel: &str) -> Result<u64, String> {
    let p = resolve(root, rel)?;
    if p.components().count() <= root.components().count() + 1 {
        return Err("choose a file inside a date folder".into());
    }
    let md = std::fs::symlink_metadata(&p).map_err(|e| format!("{rel}: {e}"))?;
    let mut freed = 0;
    if md.is_dir() {
        // a capture folder: frames of a PNG burst, or an HDR / prominence session with one sub-folder per lane
        fn rm(p: &Path, freed: &mut u64) -> Result<(), String> {
            for e in std::fs::read_dir(p).map_err(|e| e.to_string())?.flatten() {
                let m = std::fs::symlink_metadata(e.path()).map_err(|e| e.to_string())?;
                if m.is_dir() {
                    rm(&e.path(), freed)?;
                } else {
                    *freed += m.len();
                    std::fs::remove_file(e.path()).map_err(|e| e.to_string())?;
                }
            }
            std::fs::remove_dir(p).map_err(|e| format!("folder not empty or in use: {e}"))
        }
        rm(&p, &mut freed)?;
    } else {
        freed += md.len();
        std::fs::remove_file(&p).map_err(|e| e.to_string())?;
    }
    let side = p.with_extension("json");
    if side != p && side.is_file() {
        let _ = std::fs::remove_file(side);
    }
    Ok(freed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_ser(path: &Path, w: u32, h: u32, frames: &[u16]) {
        let mut b = vec![0u8; HEADER as usize];
        b[..14].copy_from_slice(b"LUCAM-RECORDER");
        b[26..30].copy_from_slice(&w.to_le_bytes());
        b[30..34].copy_from_slice(&h.to_le_bytes());
        b[34..38].copy_from_slice(&16u32.to_le_bytes());
        b[38..42].copy_from_slice(&(frames.len() as u32).to_le_bytes());
        for v in frames {
            for _ in 0..w * h {
                b.extend_from_slice(&v.to_le_bytes());
            }
        }
        for i in 0..frames.len() as u64 {
            b.extend_from_slice(&(i * 100_000).to_le_bytes()); // 10 ms apart = 100 fps
        }
        std::fs::write(path, b).unwrap();
    }

    #[test]
    fn dark_and_bright_recordings_are_told_apart() {
        let d = std::env::temp_dir().join(format!("rf-files-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        write_ser(&d.join("dark.ser"), 16, 16, &[100; 10]);
        write_ser(&d.join("bright.ser"), 16, 16, &[30000; 10]);
        let dk = ser_stats(&d.join("dark.ser"), 5).unwrap();
        let br = ser_stats(&d.join("bright.ser"), 5).unwrap();
        assert!(dk.mostly_dark && !br.mostly_dark);
        assert!(dk.complete && (dk.fps.unwrap() - 100.0).abs() < 0.5, "{:?}", dk.fps);
        assert!((br.mean_avg - 30000.0 / 65535.0).abs() < 0.01);
        // a cut-off file is reported incomplete
        let mut b = std::fs::read(d.join("bright.ser")).unwrap();
        b.truncate(HEADER as usize + 5 * 16 * 16 * 2);
        std::fs::write(d.join("cut.ser"), b).unwrap();
        let c = ser_stats(&d.join("cut.ser"), 3).unwrap();
        assert!(!c.complete && c.frames_present == 5);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn delete_stays_inside_the_folder() {
        let d = std::env::temp_dir().join(format!("rf-del-{}", std::process::id()));
        std::fs::create_dir_all(d.join("day")).unwrap();
        std::fs::write(d.join("day/a.ser"), b"x").unwrap();
        std::fs::write(d.join("day/a.json"), b"{}").unwrap();
        std::fs::write(d.join("keep.txt"), b"x").unwrap();
        assert!(delete(&d, "../x").is_err());
        assert!(delete(&d, "/etc/passwd").is_err());
        assert!(delete(&d, "day").is_err(), "date folders are not deleted");
        assert_eq!(delete(&d, "day/a.ser").unwrap(), 1);
        assert!(!d.join("day/a.ser").exists() && !d.join("day/a.json").exists() && d.join("keep.txt").exists());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_lane_session_folder_is_deleted_with_its_lanes() {
        let d = std::env::temp_dir().join(format!("rf-del-lanes-{}", std::process::id()));
        std::fs::create_dir_all(d.join("day/hdr_1/lane-a")).unwrap();
        std::fs::write(d.join("day/hdr_1/lane-a/a_c001.ser"), b"xyz").unwrap();
        std::fs::write(d.join("day/hdr_1/session.json"), b"{}").unwrap();
        assert_eq!(delete(&d, "day/hdr_1").unwrap(), 5);
        assert!(!d.join("day/hdr_1").exists());
        assert!(d.join("day").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
