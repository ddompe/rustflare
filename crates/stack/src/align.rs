//! Translation between frames by (partially whitened) phase correlation on binned, high-passed images.

use crate::image::{box_blur, Img};
use crate::fft2::{Real2d, C};


/// Radius (binned pixels) of the blur that tapers the correlation window and of the high-pass that removes the glow.
const TAPER: usize = 10;
const HIGHPASS: usize = 14;

/// A frame prepared for correlation: windowed and transformed.
pub struct Prepared {
    spec: Vec<C>,
}

/// Plans and window for one image size; build once per thread.
pub struct Aligner {
    /// The size of the images given in, and of the (centred) crop that is transformed: FFTs are fast for sizes made of small primes only.
    full_w: usize,
    full_h: usize,
    w: usize,
    h: usize,
    window: Vec<f32>,
    fft: Real2d,
    /// 0 = plain cross-correlation, 1 = full phase correlation.
    whiten: f32,
}

/// The largest size not above `n` whose only prime factors are 2, 3 and 5.
fn smooth_size(n: usize) -> usize {
    (1..=n).rev().find(|&m| { let mut r = m; for p in [2, 3, 5] { while r % p == 0 { r /= p; } } r == 1 }).unwrap_or(n)
}

impl Aligner {
    /// `valid` marks pixels with signal (see `image::valid_mask`); the window fades the correlation out of the rest.
    pub fn new(full_w: usize, full_h: usize, valid_full: &[bool], whiten: f32) -> Aligner {
        let (w, h) = (smooth_size(full_w), smooth_size(full_h));
        let (ox, oy) = ((full_w - w) / 2, (full_h - h) / 2);
        let valid: Vec<bool> = (0..w * h).map(|i| valid_full[(i / w + oy) * full_w + i % w + ox]).collect();
        let valid = &valid[..];
        let hann = |i: usize, n: usize| 0.5 - 0.5 * (std::f32::consts::TAU * (i as f32 + 0.5) / n as f32).cos();
        // a smooth taper instead of a hard edge: a static edge next to bright glow would pull every shift towards zero
        let m: Vec<f32> = valid.iter().map(|&v| if v { 1.0 } else { 0.0 }).collect();
        let taper = box_blur(&box_blur(&m, w, h, TAPER), w, h, TAPER);
        let mut window = vec![0.0f32; w * h];
        for y in 0..h {
            for x in 0..w {
                window[y * w + x] = hann(x, w) * hann(y, h) * taper[y * w + x] * m[y * w + x];
            }
        }
        Aligner { full_w, full_h, w, h, window, fft: Real2d::new(w, h), whiten }
    }

    pub fn prepare(&self, img: &Img) -> Prepared {
        assert_eq!((img.w, img.h), (self.full_w, self.full_h));
        let (ox, oy) = ((self.full_w - self.w) / 2, (self.full_h - self.h) / 2);
        let pix = |i: usize| img.d[(i / self.w + oy) * self.full_w + i % self.w + ox];
        // high-pass against the local average (over the signal only), so the broad glow does not dominate; then the window
        let wt: Vec<f32> = self.window.iter().map(|&w| if w > 0.0 { 1.0 } else { 0.0 }).collect();
        let num = box_blur(&(0..self.w * self.h).map(|i| pix(i) * wt[i]).collect::<Vec<_>>(), self.w, self.h, HIGHPASS);
        let den = box_blur(&wt, self.w, self.h, HIGHPASS);
        let mut buf: Vec<f32> = (0..self.w * self.h).map(|i| if den[i] > 1e-3 { (pix(i) - num[i] / den[i]) * self.window[i] } else { 0.0 }).collect();
        Prepared { spec: self.fft.forward(&mut buf) }
    }

    /// How far `img`'s content is displaced from `reference`'s, in (binned) pixels: `img(x, y)` is about `reference(x - dx, y - dy)`.
    pub fn shift(&self, reference: &Prepared, img: &Prepared) -> (f64, f64) {
        let cross: Vec<C> = reference.spec.iter().zip(&img.spec).map(|(r, i)| {
            let c = r.conj() * i;
            let m = c.norm();
            if m < 1e-12 { C::new(0.0, 0.0) } else { c / m.powf(self.whiten) }
        }).collect();
        let corr = self.fft.inverse(cross);
        let (w, h) = (self.w, self.h);
        let (mut best, mut bi) = (f32::MIN, 0usize);
        for (i, &v) in corr.iter().enumerate() {
            if v > best {
                best = v;
                bi = i;
            }
        }
        let (px, py) = (bi % w, bi / w);
        let at = |x: isize, y: isize| corr[(y.rem_euclid(h as isize) as usize) * w + x.rem_euclid(w as isize) as usize] as f64;
        let sub = |m: f64, c: f64, p: f64| {
            let d = m - 2.0 * c + p;
            if d.abs() < 1e-12 { 0.0 } else { (0.5 * (m - p) / d).clamp(-0.5, 0.5) }
        };
        let (cx, cy) = (px as isize, py as isize);
        let c = at(cx, cy);
        let fx = sub(at(cx - 1, cy), c, at(cx + 1, cy));
        let fy = sub(at(cx, cy - 1), c, at(cx, cy + 1));
        let wrap = |p: usize, n: usize| if p > n / 2 { p as f64 - n as f64 } else { p as f64 };
        (wrap(px, w) + fx, wrap(py, h) + fy)
    }
}

/// Add `src` shifted by (-dx, -dy) (so content displaced by (dx, dy) lands back on the reference) into `acc`, bilinear.
pub fn accumulate_shifted(acc: &mut [f32], src: &[f32], w: usize, h: usize, dx: f64, dy: f64) {
    crate::simd::accumulate_shifted(acc, src, w, h, dx, dy)
}
