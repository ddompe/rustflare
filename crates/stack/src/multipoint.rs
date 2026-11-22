//! Multi-point alignment: a grid of small boxes ("alignment points") on the structured parts of the image, each with its own
//! shift per frame and its own choice of the best frames. Seeing bends the image differently a few arcseconds apart, so one
//! shift per frame leaves the detail smeared; a shift per point does not.
//!
//! The pieces here are the grid, the local correlation and the blend; `stack.rs` runs the passes.

use crate::image::Img;
use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::sync::Arc;

type C = Complex<f32>;

/// Centres of the alignment points (in the coordinates of the image they were built on, usually 2x binned).
///
/// A point is kept when its whole box lies on signal and its reference patch has enough detail: the Laplacian energy must reach
/// `min_structure` times the 90th percentile of all candidates (so smooth glow around a prominence is dropped, a uniformly
/// textured disc keeps everything).
pub fn build_grid(reference: &Img, valid: &[bool], half: usize, pitch: usize, min_structure: f64) -> Vec<(usize, usize)> {
    let (w, h) = (reference.w, reference.h);
    let mut cand: Vec<((usize, usize), f64)> = Vec::new();
    let mut y = half;
    while y + half <= h {
        let mut x = half;
        while x + half <= w {
            let on = (y - half..y + half).step_by(4).all(|yy| (x - half..x + half).step_by(4).all(|xx| valid[yy * w + xx]));
            if on {
                let mut e = 0.0f64;
                for yy in y - half + 1..y + half - 1 {
                    for xx in x - half + 1..x + half - 1 {
                        let i = yy * w + xx;
                        let l = 4.0 * reference.d[i] - reference.d[i - 1] - reference.d[i + 1] - reference.d[i - w] - reference.d[i + w];
                        e += (l * l) as f64;
                    }
                }
                cand.push(((x, y), e));
            }
            x += pitch;
        }
        y += pitch;
    }
    if cand.is_empty() {
        return Vec::new();
    }
    let mut energies: Vec<f64> = cand.iter().map(|c| c.1).collect();
    energies.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let p90 = energies[(energies.len() * 9 / 10).min(energies.len() - 1)];
    cand.into_iter().filter(|c| c.1 >= min_structure * p90 && c.1 > 0.0).map(|c| c.0).collect()
}

/// A patch prepared for correlation.
pub struct Patch {
    spec: Vec<C>,
    norm: f32,
}

/// Local cross-correlation of 2*half boxes, with a search of +-`search` pixels.
pub struct LocalAligner {
    half: usize,
    n: usize,
    search: usize,
    window: Vec<f32>,
    fwd: Arc<dyn Fft<f32>>,
    inv: Arc<dyn Fft<f32>>,
}

fn transpose(src: &[C], n: usize) -> Vec<C> {
    let mut out = vec![C::new(0.0, 0.0); n * n];
    for y in 0..n {
        for x in 0..n {
            out[x * n + y] = src[y * n + x];
        }
    }
    out
}

impl LocalAligner {
    pub fn new(half: usize, search: usize) -> LocalAligner {
        let n = (2 * half + 2 * search + 7) / 8 * 8; // room so that a shift of `search` does not wrap onto the peak
        let mut p = FftPlanner::<f32>::new();
        let hann = |i: usize| (std::f32::consts::PI * (i as f32 + 0.5) / (2 * half) as f32).sin().powi(2);
        let mut window = vec![0f32; 4 * half * half];
        for y in 0..2 * half {
            for x in 0..2 * half {
                window[y * 2 * half + x] = hann(x) * hann(y);
            }
        }
        LocalAligner { half, n, search, window, fwd: p.plan_fft_forward(n), inv: p.plan_fft_inverse(n) }
    }

    /// The box around (`cx`, `cy`) of `img` (`w` wide, edges clamp), made zero-mean and windowed.
    pub fn prepare(&self, img: &[f32], w: usize, h: usize, cx: isize, cy: isize) -> Patch {
        let (hf, n) = (self.half, self.n);
        let side = 2 * hf;
        let mut vals = vec![0f32; side * side];
        for y in 0..side {
            let yy = (cy - hf as isize + y as isize).clamp(0, h as isize - 1) as usize;
            for x in 0..side {
                let xx = (cx - hf as isize + x as isize).clamp(0, w as isize - 1) as usize;
                vals[y * side + x] = img[yy * w + xx];
            }
        }
        let mean = vals.iter().map(|&v| v as f64).sum::<f64>() / vals.len() as f64;
        let mut buf = vec![C::new(0.0, 0.0); n * n];
        let off = (n - side) / 2;
        let mut sq = 0.0f64;
        for y in 0..side {
            for x in 0..side {
                let v = (vals[y * side + x] - mean as f32) * self.window[y * side + x];
                sq += (v * v) as f64;
                buf[(y + off) * n + x + off] = C::new(v, 0.0);
            }
        }
        for row in buf.chunks_exact_mut(n) {
            self.fwd.process(row);
        }
        let mut t = transpose(&buf, n);
        for row in t.chunks_exact_mut(n) {
            self.fwd.process(row);
        }
        Patch { spec: t, norm: sq.sqrt() as f32 }
    }

    /// How far `img`'s content is displaced from `reference`'s (`img(x) ~ reference(x - d)`), and the normalised correlation at
    /// the peak (1 = identical up to the shift; falls with blur and noise).
    pub fn correlate(&self, reference: &Patch, img: &Patch) -> (f32, f32, f32) {
        let n = self.n;
        let mut cross: Vec<C> = reference.spec.iter().zip(&img.spec).map(|(r, i)| r.conj() * i).collect();
        for row in cross.chunks_exact_mut(n) {
            self.inv.process(row);
        }
        let mut t = transpose(&cross, n);
        for row in t.chunks_exact_mut(n) {
            self.inv.process(row);
        }
        let at = |x: isize, y: isize| t[(y.rem_euclid(n as isize) as usize) * n + x.rem_euclid(n as isize) as usize].re / (n * n) as f32;
        let (mut best, mut bx, mut by) = (f32::MIN, 0isize, 0isize);
        let s = self.search as isize;
        for dy in -s..=s {
            for dx in -s..=s {
                let v = at(dx, dy);
                if v > best {
                    (best, bx, by) = (v, dx, dy);
                }
            }
        }
        let sub = |m: f32, c: f32, p: f32| {
            let d = m - 2.0 * c + p;
            if d.abs() < 1e-12 { 0.0 } else { (0.5 * (m - p) / d).clamp(-0.5, 0.5) }
        };
        let fx = sub(at(bx - 1, by), best, at(bx + 1, by));
        let fy = sub(at(bx, by - 1), best, at(bx, by + 1));
        let denom = reference.norm * img.norm;
        (bx as f32 + fx, by as f32 + fy, if denom > 0.0 { (best / denom).clamp(-1.0, 1.0) } else { 0.0 })
    }
}

fn hann(i: usize, n: usize) -> f32 { (std::f32::consts::PI * (i as f32 + 0.5) / n as f32).sin().powi(2) }

/// Add the box (`2*half` wide, centred on `cx`, `cy` in the output) of `frame` sampled at a displacement (`dx`, `dy`) into the
/// weighted sums: `num += w * value`, `den += w`, with a Hann window as the weight so neighbouring boxes blend smoothly.
pub fn add_box(num: &mut [f32], den: &mut [f32], frame: &[f32], w: usize, h: usize, cx: isize, cy: isize, half: usize, dx: f32, dy: f32) {
    let side = 2 * half;
    let (ix, iy) = (dx.floor(), dy.floor());
    let (fx, fy) = (dx - ix, dy - iy);
    let (ix, iy) = (ix as isize, iy as isize);
    let at = |x: isize, y: isize| frame[(y.clamp(0, h as isize - 1) as usize) * w + x.clamp(0, w as isize - 1) as usize];
    for y in 0..side {
        let ty = cy - half as isize + y as isize;
        if ty < 0 || ty >= h as isize {
            continue;
        }
        let wy = hann(y, side);
        for x in 0..side {
            let tx = cx - half as isize + x as isize;
            if tx < 0 || tx >= w as isize {
                continue;
            }
            let (sx, sy) = (tx + ix, ty + iy);
            let v = at(sx, sy) * (1.0 - fx) * (1.0 - fy) + at(sx + 1, sy) * fx * (1.0 - fy) + at(sx, sy + 1) * (1.0 - fx) * fy + at(sx + 1, sy + 1) * fx * fy;
            let wt = wy * hann(x, side);
            let i = ty as usize * w + tx as usize;
            num[i] += wt * v;
            den[i] += wt;
        }
    }
}
