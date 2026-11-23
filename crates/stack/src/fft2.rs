//! 2D FFT of a real image, exploiting that the image is real: rows are real-to-complex (half the spectrum), then complex columns.
//! About 2x faster than a complex 2D FFT of the same size on a Pi 5 (docs/project/stacker-simd-gpu-spike-2026-11-22.md, FFT spike).
//!
//! The spectrum is `(w / 2 + 1)` columns of `h` values: element `[kx * h + ky]`.

use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};
use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::sync::Arc;

pub type C = Complex<f32>;

/// Out-of-place transpose in tiles (a plain double loop is cache-hostile on big images): `out[x * h + y] = src[y * w + x]`.
pub fn transpose<T: Copy>(src: &[T], w: usize, h: usize, out: &mut [T]) {
    const B: usize = 16;
    for by in (0..h).step_by(B) {
        for bx in (0..w).step_by(B) {
            for y in by..(by + B).min(h) {
                for x in bx..(bx + B).min(w) {
                    out[x * h + y] = src[y * w + x];
                }
            }
        }
    }
}

pub struct Real2d {
    pub w: usize,
    pub h: usize,
    r2c: Arc<dyn RealToComplex<f32>>,
    c2r: Arc<dyn ComplexToReal<f32>>,
    fh: Arc<dyn Fft<f32>>,
    ih: Arc<dyn Fft<f32>>,
}

impl Real2d {
    pub fn new(w: usize, h: usize) -> Real2d {
        assert!(w % 2 == 0, "the width must be even");
        let (mut rp, mut p) = (RealFftPlanner::<f32>::new(), FftPlanner::<f32>::new());
        Real2d { w, h, r2c: rp.plan_fft_forward(w), c2r: rp.plan_fft_inverse(w), fh: p.plan_fft_forward(h), ih: p.plan_fft_inverse(h) }
    }

    pub fn spectrum_len(&self) -> usize { (self.w / 2 + 1) * self.h }

    /// Forward transform of a `w x h` real image (the input is used as scratch and left changed).
    pub fn forward(&self, img: &mut [f32]) -> Vec<C> {
        let hw = self.w / 2 + 1;
        let mut half = vec![C::new(0.0, 0.0); hw * self.h];
        let mut scratch = vec![C::new(0.0, 0.0); self.r2c.get_scratch_len()];
        for (y, row) in img.chunks_exact_mut(self.w).enumerate() {
            self.r2c.process_with_scratch(row, &mut half[y * hw..(y + 1) * hw], &mut scratch).expect("sizes are fixed by construction");
        }
        let mut t = vec![C::new(0.0, 0.0); hw * self.h];
        transpose(&half, hw, self.h, &mut t);
        for col in t.chunks_exact_mut(self.h) {
            self.fh.process(col);
        }
        t
    }

    /// Inverse transform back to a `w x h` real image (not divided by `w * h`).
    pub fn inverse(&self, mut spec: Vec<C>) -> Vec<f32> {
        let hw = self.w / 2 + 1;
        for col in spec.chunks_exact_mut(self.h) {
            self.ih.process(col);
        }
        let mut half = vec![C::new(0.0, 0.0); hw * self.h];
        transpose(&spec, self.h, hw, &mut half);
        let mut out = vec![0f32; self.w * self.h];
        let mut scratch = vec![C::new(0.0, 0.0); self.c2r.get_scratch_len()];
        for (y, row) in half.chunks_exact_mut(hw).enumerate() {
            // the imaginary parts of the first and last bin of a real signal's spectrum are zero up to rounding; realfft reports them
            // as an error but still transforms
            let _ = self.c2r.process_with_scratch(row, &mut out[y * self.w..(y + 1) * self.w], &mut scratch);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_real_image_comes_back_after_forward_and_inverse_and_matches_the_complex_transform() {
        let (w, h) = (24usize, 18usize);
        let mut s = 3u32;
        let img: Vec<f32> = (0..w * h).map(|_| { s = s.wrapping_mul(1664525).wrapping_add(1013904223); (s >> 8) as f32 / 16777216.0 - 0.5 }).collect();
        let f = Real2d::new(w, h);
        let spec = f.forward(&mut img.clone());
        // the half spectrum equals the left half of the complex 2D transform
        let mut p = FftPlanner::<f32>::new();
        let (fw, fh) = (p.plan_fft_forward(w), p.plan_fft_forward(h));
        let mut full: Vec<C> = img.iter().map(|&v| C::new(v, 0.0)).collect();
        for r in full.chunks_exact_mut(w) { fw.process(r); }
        let mut t = vec![C::new(0.0, 0.0); w * h];
        transpose(&full, w, h, &mut t);
        for c in t.chunks_exact_mut(h) { fh.process(c); }
        let hw = w / 2 + 1;
        let worst = (0..hw * h).map(|i| (spec[i] - t[i]).norm()).fold(0.0f32, f32::max);
        assert!(worst < 1e-3, "half spectrum differs by {worst}");
        let back = f.inverse(spec);
        let worst = back.iter().zip(&img).map(|(a, b)| (a / (w * h) as f32 - b).abs()).fold(0.0f32, f32::max);
        assert!(worst < 1e-5, "round trip differs by {worst}");
    }
}
