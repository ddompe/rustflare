//! JPEG preview encoding on its own thread so it never delays capture.

use rustflare_core::{Frame, PixelFormat};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::watch;

const MAX_WIDTH: u32 = 1280;

/// Software coronagraph for the live view: black out the Sun's disc so the stretch is set by the faint prominences and
/// limb structure around it. Display only; recordings and tracking use the untouched frame.
#[derive(Default)]
pub struct Corona {
    pub enabled: bool,
    /// Mask radius as a fraction of the detected Sun radius (a little over 1 hides the bright limb, which would otherwise set the white point).
    pub scale: f64,
    /// Latest detected Sun (x, y, radius) in frame pixels, set by the engine.
    pub sun: Option<(f64, f64, f64)>,
}
pub type SharedCorona = Arc<std::sync::Mutex<Corona>>;

/// Sets every pixel inside the circle to zero.
pub fn mask_disc(frame: &mut Frame, cx: f64, cy: f64, r: f64) {
    let (w, h) = (frame.width as i64, frame.height as i64);
    let bpp = frame.format.bytes_per_pixel();
    let y0 = ((cy - r).floor() as i64).max(0);
    let y1 = ((cy + r).ceil() as i64).min(h - 1);
    for y in y0..=y1 {
        let dy = y as f64 - cy;
        let half = (r * r - dy * dy).max(0.0).sqrt();
        let x0 = ((cx - half).ceil() as i64).max(0);
        let x1 = ((cx + half).floor() as i64).min(w - 1);
        if x1 < x0 {
            continue;
        }
        let a = (y * w + x0) as usize * bpp;
        let b = (y * w + x1 + 1) as usize * bpp;
        frame.data[a..b].fill(0);
    }
}

/// Who is looking at the picture. With nobody watching (an unattended recording) the engine skips the preview copy,
/// the downscale and the JPEG encode entirely, about a quarter of a Pi core.
#[derive(Default)]
pub struct Demand {
    viewers: std::sync::atomic::AtomicUsize,
    snapshot_until_ms: std::sync::atomic::AtomicU64,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

impl Demand {
    pub fn wanted(&self) -> bool {
        self.viewers.load(Ordering::Relaxed) > 0 || now_ms() < self.snapshot_until_ms.load(Ordering::Relaxed)
    }
    /// A one-off still (`/snapshot.jpg`) keeps the preview running for a few seconds.
    pub fn want_snapshot(&self) {
        self.snapshot_until_ms.store(now_ms() + 3000, Ordering::Relaxed);
    }
    /// Counts a live viewer until the returned guard is dropped.
    pub fn viewer(self: &Arc<Self>) -> ViewerGuard {
        self.viewers.fetch_add(1, Ordering::Relaxed);
        ViewerGuard(self.clone())
    }
}

pub struct ViewerGuard(Arc<Demand>);
impl Drop for ViewerGuard {
    fn drop(&mut self) {
        self.0.viewers.fetch_sub(1, Ordering::Relaxed);
    }
}

pub type JpegTx = watch::Sender<Option<Arc<Vec<u8>>>>;

/// Display range (in the frame's own units) that maps to black..white when auto-stretching:
/// the 0.5th and 99.8th percentile, so a few hot pixels or dark corners don't flatten the picture.
fn stretch_range(frame: &Frame, skip_zero: bool) -> (f64, f64) {
    let full = frame.format.full_scale();
    let mut hist = vec![0u32; 4096];
    let mut n = 0u64;
    for y in (0..frame.height).step_by(4) {
        for x in (0..frame.width).step_by(4) {
            let v = frame.pixel(x, y);
            if skip_zero && v == 0 {
                continue;
            }
            hist[((v as f64 / (full + 1.0)) * 4096.0) as usize] += 1;
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

pub fn encode(frame: &Frame, stretch: bool, masked: bool) -> Option<Vec<u8>> {
    let f = frame.width.div_ceil(MAX_WIDTH).max(1) as usize;
    let (w, h) = (frame.width as usize / f, frame.height as usize / f);
    let (lo, hi) = if stretch { stretch_range(frame, masked) } else { (0.0, frame.format.full_scale() + 1.0) };
    // Fixed point (16.16) multiplier: one integer multiply and shift per output pixel, no floating point in the loop.
    let lo_i = lo as i64;
    let mul = ((255.0 / (hi - lo).max(1.0)) * 65536.0) as i64;
    let stride = frame.width as usize;
    let mut gray = Vec::with_capacity(w * h);
    match frame.format {
        PixelFormat::Mono16 => {
            for y in 0..h {
                let row = &frame.data[y * f * stride * 2..(y * f + 1) * stride * 2];
                if stretch {
                    gray.extend((0..w).map(|x| {
                        let v = u16::from_le_bytes([row[x * f * 2], row[x * f * 2 + 1]]) as i64;
                        (((v - lo_i).max(0) * mul) >> 16).min(255) as u8
                    }));
                } else {
                    // without stretching: the plain high byte (cheap and exact)
                    gray.extend((0..w).map(|x| row[x * f * 2 + 1]));
                }
            }
        }
        PixelFormat::Mono8 => {
            for y in 0..h {
                let row = &frame.data[y * f * stride..(y * f + 1) * stride];
                if stretch {
                    gray.extend((0..w).map(|x| (((row[x * f] as i64 - lo_i).max(0) * mul) >> 16).min(255) as u8));
                } else {
                    gray.extend((0..w).map(|x| row[x * f]));
                }
            }
        }
    }
    let mut out = Vec::new();
    jpeg_encoder::Encoder::new(&mut out, 75).encode(&gray, w as u16, h as u16, jpeg_encoder::ColorType::Luma).ok()?;
    Some(out)
}

/// Returns a sender; frames are dropped (never queued) while the encoder is busy.
pub fn spawn(tx: JpegTx, stretch: Arc<AtomicBool>, flat: rustflare_capture::SharedFlat, corona: SharedCorona) -> SyncSender<Frame> {
    let (ftx, frx) = sync_channel::<Frame>(1);
    std::thread::Builder::new()
        .name("preview".into())
        .spawn(move || {
            while let Ok(mut frame) = frx.recv() {
                // flat-field correction is for display only: recordings stay raw
                let (dark, field) = { let s = flat.lock().unwrap(); (if s.dark_enabled { s.dark.clone() } else { None }, if s.enabled { s.field.clone() } else { None }) };
                if let Some(dk) = dark.filter(|d| d.matches(&frame)) {
                    dk.apply(&mut frame);
                }
                if let Some(ff) = field.filter(|ff| ff.matches(&frame)) {
                    ff.apply(&mut frame);
                }
                let mask = { let c = corona.lock().unwrap(); if c.enabled { c.sun.map(|(x, y, r)| (x, y, r * c.scale)) } else { None } };
                if let Some((x, y, r)) = mask {
                    mask_disc(&mut frame, x, y, r);
                }
                // Not forced to stretch: the point of the mask is to judge the exposure, so the picture must keep showing
                // the real brightness unless the viewer chose auto-stretch (which would hide any exposure change).
                let st = stretch.load(Ordering::Relaxed);
                if let Some(jpg) = encode(&frame, st, mask.is_some()) {
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
        let (lo, hi) = stretch_range(&f, false);
        assert!(lo >= 150.0 && lo <= 250.0 && hi >= 1900.0 && hi <= 2100.0, "{lo} {hi}");
        assert!(encode(&f, true, false).is_some() && encode(&f, false, false).is_some());
    }

    /// `cargo test --release -p rustflare-server bench_encode -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn bench_encode() {
        let (w, h) = (1944u32, 1472u32);
        let d: Vec<u8> = (0..w * h).flat_map(|i| (((i % 1000) * 40) as u16).to_le_bytes()).collect();
        let f = Frame::new(w, h, PixelFormat::Mono16, d);
        for stretch in [false, true] {
            let t = std::time::Instant::now();
            for _ in 0..20 { assert!(encode(&f, stretch, false).is_some()); }
            println!("encode stretch={stretch}: {:.1} ms/frame", t.elapsed().as_secs_f64() * 50.0);
        }
        let t = std::time::Instant::now();
        for _ in 0..20 { let _ = f.clone(); }
        println!("clone: {:.1} ms", t.elapsed().as_secs_f64() * 50.0);
    }

    #[test]
    fn coronagraph_masks_the_disc_and_stretches_the_rest() {
        // bright disc (60000) on a faint sky (300) with a faint "prominence" ring (900)
        let (w, h) = (64u32, 64u32);
        let mut d = Vec::new();
        for y in 0..h { for x in 0..w {
            let r2 = (x as i32 - 32).pow(2) + (y as i32 - 32).pow(2);
            let v: u16 = if r2 < 200 { 60000 } else if r2 < 260 { 900 } else { 300 };
            d.extend_from_slice(&v.to_le_bytes());
        } }
        let mut f = Frame::new(w, h, PixelFormat::Mono16, d);
        mask_disc(&mut f, 32.0, 32.0, 14.2);
        assert_eq!(f.pixel(32, 32), 0);
        assert_eq!(f.pixel(0, 0), 300);
        let (lo, hi) = stretch_range(&f, true);
        assert!(hi < 2000.0 && lo >= 250.0, "{lo} {hi}: the disc must not set the white point");
    }
}
