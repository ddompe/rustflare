//! Translation between frames by (partially whitened) phase correlation on binned, high-passed images.

use crate::image::{box_blur, Img};
use rustfft::{num_complex::Complex, FftPlanner};
use std::sync::Arc;

type C = Complex<f32>;

/// Radius (binned pixels) of the blur that tapers the correlation window and of the high-pass that removes the glow.
const TAPER: usize = 10;
const HIGHPASS: usize = 14;

/// A frame prepared for correlation: windowed and transformed.
pub struct Prepared {
    spec: Vec<C>,
}

/// Plans and window for one image size; build once per thread.
pub struct Aligner {
    w: usize,
    h: usize,
    window: Vec<f32>,
    fw: Arc<dyn rustfft::Fft<f32>>,
    fh: Arc<dyn rustfft::Fft<f32>>,
    iw: Arc<dyn rustfft::Fft<f32>>,
    ih: Arc<dyn rustfft::Fft<f32>>,
    /// 0 = plain cross-correlation, 1 = full phase correlation.
    whiten: f32,
}

fn transpose(src: &[C], w: usize, h: usize) -> Vec<C> {
    let mut out = vec![C::new(0.0, 0.0); w * h];
    for y in 0..h {
        for x in 0..w {
            out[x * h + y] = src[y * w + x];
        }
    }
    out
}

impl Aligner {
    /// `valid` marks pixels with signal (see `image::valid_mask`); the window fades the correlation out of the rest.
    pub fn new(w: usize, h: usize, valid: &[bool], whiten: f32) -> Aligner {
        let mut p = FftPlanner::<f32>::new();
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
        Aligner { w, h, window, fw: p.plan_fft_forward(w), fh: p.plan_fft_forward(h), iw: p.plan_fft_inverse(w), ih: p.plan_fft_inverse(h), whiten }
    }

    /// Spectrum in [w rows][h columns] layout.
    fn forward(&self, mut buf: Vec<C>) -> Vec<C> {
        for row in buf.chunks_exact_mut(self.w) {
            self.fw.process(row);
        }
        let mut t = transpose(&buf, self.w, self.h);
        for row in t.chunks_exact_mut(self.h) {
            self.fh.process(row);
        }
        t
    }

    fn inverse(&self, mut spec: Vec<C>) -> Vec<f32> {
        for row in spec.chunks_exact_mut(self.h) {
            self.ih.process(row);
        }
        let mut t = transpose(&spec, self.h, self.w);
        for row in t.chunks_exact_mut(self.w) {
            self.iw.process(row);
        }
        t.iter().map(|c| c.re).collect()
    }

    pub fn prepare(&self, img: &Img) -> Prepared {
        assert_eq!((img.w, img.h), (self.w, self.h));
        // high-pass against the local average (over the signal only), so the broad glow does not dominate; then the window
        let wt: Vec<f32> = self.window.iter().map(|&w| if w > 0.0 { 1.0 } else { 0.0 }).collect();
        let num = box_blur(&img.d.iter().zip(&wt).map(|(v, m)| v * m).collect::<Vec<_>>(), self.w, self.h, HIGHPASS);
        let den = box_blur(&wt, self.w, self.h, HIGHPASS);
        let buf: Vec<C> = (0..self.w * self.h).map(|i| C::new(if den[i] > 1e-3 { (img.d[i] - num[i] / den[i]) * self.window[i] } else { 0.0 }, 0.0)).collect();
        Prepared { spec: self.forward(buf) }
    }

    /// How far `img`'s content is displaced from `reference`'s, in (binned) pixels: `img(x, y)` is about `reference(x - dx, y - dy)`.
    pub fn shift(&self, reference: &Prepared, img: &Prepared) -> (f64, f64) {
        let cross: Vec<C> = reference.spec.iter().zip(&img.spec).map(|(r, i)| {
            let c = r.conj() * i;
            let m = c.norm();
            if m < 1e-12 { C::new(0.0, 0.0) } else { c / m.powf(self.whiten) }
        }).collect();
        let corr = self.inverse(cross);
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
    let (ix, iy) = (dx.floor(), dy.floor());
    let (fx, fy) = ((dx - ix) as f32, (dy - iy) as f32);
    let (ix, iy) = (ix as isize, iy as isize);
    let at = |x: isize, y: isize| src[(y.clamp(0, h as isize - 1) as usize) * w + x.clamp(0, w as isize - 1) as usize];
    for y in 0..h as isize {
        for x in 0..w as isize {
            let (sx, sy) = (x + ix, y + iy);
            let v = at(sx, sy) * (1.0 - fx) * (1.0 - fy) + at(sx + 1, sy) * fx * (1.0 - fy) + at(sx, sy + 1) * (1.0 - fx) * fy + at(sx + 1, sy + 1) * fx * fy;
            acc[y as usize * w + x as usize] += v;
        }
    }
}
