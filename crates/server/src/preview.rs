//! JPEG preview encoding on its own thread so it never delays capture.

use rustflare_core::{Frame, PixelFormat};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::watch;

const MAX_WIDTH: u32 = 1280;

pub type JpegTx = watch::Sender<Option<Arc<Vec<u8>>>>;

/// Display range (in the frame's own units) that maps to black..white when auto-stretching:
/// the 0.5th and 99.8th percentile, so a few hot pixels or dark corners don't flatten the picture.
fn stretch_range(frame: &Frame) -> (f64, f64) {
    let full = frame.format.full_scale();
    let mut hist = vec![0u32; 4096];
    let mut n = 0u64;
    for y in (0..frame.height).step_by(4) {
        for x in (0..frame.width).step_by(4) {
            hist[((frame.pixel(x, y) as f64 / (full + 1.0)) * 4096.0) as usize] += 1;
            n += 1;
        }
    }
    let pick = |frac: f64| {
        let target = (n as f64 * frac) as u64;
        let mut acc = 0u64;
        for (i, c) in hist.iter().enumerate() {
            acc += *c as u64;
            if acc >= target {
                return i as f64 / 4096.0 * (full + 1.0);
            }
        }
        full
    };
    let (lo, hi) = (pick(0.005), pick(0.998));
    if hi - lo < full / 256.0 { (lo, lo + full / 256.0) } else { (lo, hi) }
}

pub fn encode(frame: &Frame, stretch: bool) -> Option<Vec<u8>> {
    let f = frame.width.div_ceil(MAX_WIDTH).max(1);
    let (w, h) = (frame.width / f, frame.height / f);
    let (lo, hi) = if stretch { stretch_range(frame) } else { (0.0, frame.format.full_scale() + 1.0) };
    let scale = 255.0 / (hi - lo);
    // 16-bit data uses the full range when stretching; without it, the plain high byte (cheap and exact).
    let mut gray = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let v = frame.pixel(x * f, y * f) as f64;
            gray.push(if stretch {
                ((v - lo) * scale).clamp(0.0, 255.0) as u8
            } else {
                match frame.format {
                    PixelFormat::Mono8 => v as u8,
                    PixelFormat::Mono16 => (frame.pixel(x * f, y * f) >> 8) as u8,
                }
            });
        }
    }
    let mut out = Vec::new();
    jpeg_encoder::Encoder::new(&mut out, 75)
        .encode(&gray, w as u16, h as u16, jpeg_encoder::ColorType::Luma)
        .ok()?;
    Some(out)
}

/// Returns a sender; frames are dropped (never queued) while the encoder is busy.
pub fn spawn(tx: JpegTx, stretch: Arc<AtomicBool>, flat: rustflare_capture::SharedFlat) -> SyncSender<Frame> {
    let (ftx, frx) = sync_channel::<Frame>(1);
    std::thread::Builder::new()
        .name("preview".into())
        .spawn(move || {
            while let Ok(mut frame) = frx.recv() {
                // flat-field correction is for display only: recordings stay raw
                let field = { let s = flat.lock().unwrap(); if s.enabled { s.field.clone() } else { None } };
                if let Some(ff) = field.filter(|ff| ff.matches(&frame)) {
                    ff.apply(&mut frame);
                }
                if let Some(jpg) = encode(&frame, stretch.load(Ordering::Relaxed)) {
                    tx.send_replace(Some(Arc::new(jpg)));
                }
            }
        })
        .expect("spawn preview thread");
    ftx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stretch_expands_a_dim_frame() {
        // dim 16-bit frame: sky ~200, "disc" ~2000 (out of 65535): almost black without stretching
        let (w, h) = (64u32, 64u32);
        let mut d = Vec::new();
        for y in 0..h { for x in 0..w {
            let v: u16 = if (x as i32 - 32).pow(2) + (y as i32 - 32).pow(2) < 400 { 2000 } else { 200 };
            d.extend_from_slice(&v.to_le_bytes());
        } }
        let f = Frame::new(w, h, PixelFormat::Mono16, d);
        let (lo, hi) = stretch_range(&f);
        assert!(lo >= 150.0 && lo <= 250.0 && hi >= 1900.0 && hi <= 2100.0, "{lo} {hi}");
        assert!(encode(&f, true).is_some() && encode(&f, false).is_some());
    }
}
