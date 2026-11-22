//! Spike: how much do explicit NEON intrinsics buy on the stacker's per-pixel loops?
//! Run: cargo run --release -p rustflare-stack --example simd_spike   (aarch64 only for the NEON variants)

use rustflare_stack::align::accumulate_shifted;
use rustflare_stack::calib::Calibration;
use rustflare_stack::image::{bin, sharpness};
use std::sync::Arc;
use std::time::Instant;

const W: usize = 1944;
const H: usize = 1472;

fn frame(seed: u32) -> Vec<u16> {
    let mut s = seed.wrapping_mul(2654435761).wrapping_add(1);
    (0..W * H).map(|i| { s = s.wrapping_mul(1664525).wrapping_add(1013904223); (2000 + ((i % W) as u32 / 3) + (s >> 22)) as u16 }).collect()
}

/// V0: what the pipeline does today.
fn pass_a_current(raw: &[u16], cal: &Calibration, valid: &[bool]) -> (Vec<f32>, f64) {
    let img = bin(&cal.apply(raw), W, H, 2);
    let s = sharpness(&img, valid);
    (img.d, s)
}

/// V1: the same work, fused and written for the compiler to vectorise (no allocation of the full-size float frame, no bounds checks
/// inside the loops).
fn pass_a_fused_scalar(raw: &[u16], inv_flat: &[f32]) -> (Vec<f32>, f64) {
    let (ow, oh) = (W / 2, H / 2);
    let mut out = vec![0f32; ow * oh];
    for y in 0..oh {
        let (r0, r1) = (&raw[2 * y * W..(2 * y + 1) * W], &raw[(2 * y + 1) * W..(2 * y + 2) * W]);
        let (f0, f1) = (&inv_flat[2 * y * W..(2 * y + 1) * W], &inv_flat[(2 * y + 1) * W..(2 * y + 2) * W]);
        for ((((o, p0), p1), q0), q1) in out[y * ow..(y + 1) * ow].iter_mut().zip(r0.chunks_exact(2)).zip(r1.chunks_exact(2)).zip(f0.chunks_exact(2)).zip(f1.chunks_exact(2)) {
            *o = 0.25 * (p0[0] as f32 * q0[0] + p0[1] as f32 * q0[1] + p1[0] as f32 * q1[0] + p1[1] as f32 * q1[1]);
        }
    }
    let s = lap_stats_scalar(&out, ow, oh);
    (out, s)
}

fn lap_stats_scalar(img: &[f32], w: usize, h: usize) -> f64 {
    let (mut s, mut s2, mut sm, mut n) = (0.0f64, 0.0f64, 0.0f64, 0usize);
    for y in 1..h - 1 {
        let (up, mid, dn) = (&img[(y - 1) * w..y * w], &img[y * w..(y + 1) * w], &img[(y + 1) * w..(y + 2) * w]);
        let (mut rs, mut rs2, mut rm) = (0.0f32, 0.0f32, 0.0f32);
        for x in 1..w - 1 {
            let l = 4.0 * mid[x] - mid[x - 1] - mid[x + 1] - up[x] - dn[x];
            rs += l;
            rs2 += l * l;
            rm += mid[x];
        }
        s += rs as f64;
        s2 += rs2 as f64;
        sm += rm as f64;
        n += w - 2;
    }
    let nf = n as f64;
    let var = s2 / nf - (s / nf) * (s / nf);
    let mean = sm / nf;
    var / (mean * mean)
}

#[cfg(target_arch = "aarch64")]
mod neon {
    use super::{H, W};
    use std::arch::aarch64::*;

    /// V2: explicit NEON. Eight pixels of two rows per step: widen to f32, multiply by 1/flat, add the rows, pairwise-add columns.
    pub fn pass_a(raw: &[u16], inv_flat: &[f32]) -> (Vec<f32>, f64) {
        let (ow, oh) = (W / 2, H / 2);
        let mut out = vec![0f32; ow * oh];
        unsafe {
            let q = vdupq_n_f32(0.25);
            for y in 0..oh {
                let (p0, p1) = (raw.as_ptr().add(2 * y * W), raw.as_ptr().add((2 * y + 1) * W));
                let (g0, g1) = (inv_flat.as_ptr().add(2 * y * W), inv_flat.as_ptr().add((2 * y + 1) * W));
                let o = out.as_mut_ptr().add(y * ow);
                let mut x = 0;
                while x + 8 <= W {
                    let (a, b) = (vld1q_u16(p0.add(x)), vld1q_u16(p1.add(x)));
                    let a_lo = vmulq_f32(vcvtq_f32_u32(vmovl_u16(vget_low_u16(a))), vld1q_f32(g0.add(x)));
                    let a_hi = vmulq_f32(vcvtq_f32_u32(vmovl_high_u16(a)), vld1q_f32(g0.add(x + 4)));
                    let b_lo = vmulq_f32(vcvtq_f32_u32(vmovl_u16(vget_low_u16(b))), vld1q_f32(g1.add(x)));
                    let b_hi = vmulq_f32(vcvtq_f32_u32(vmovl_high_u16(b)), vld1q_f32(g1.add(x + 4)));
                    let sum = vpaddq_f32(vaddq_f32(a_lo, b_lo), vaddq_f32(a_hi, b_hi)); // (x0+x1, x2+x3, x4+x5, x6+x7) of both rows
                    vst1q_f32(o.add(x / 2), vmulq_f32(sum, q));
                    x += 8;
                }
            }
        }
        let s = lap_stats(&out, ow, oh);
        (out, s)
    }

    pub fn lap_stats(img: &[f32], w: usize, h: usize) -> f64 {
        let (mut s, mut s2, mut sm, mut n) = (0.0f64, 0.0f64, 0.0f64, 0usize);
        unsafe {
            let four = vdupq_n_f32(4.0);
            for y in 1..h - 1 {
                let (up, mid, dn) = (img.as_ptr().add((y - 1) * w), img.as_ptr().add(y * w), img.as_ptr().add((y + 1) * w));
                let (mut vs, mut vs2, mut vm) = (vdupq_n_f32(0.0), vdupq_n_f32(0.0), vdupq_n_f32(0.0));
                let mut x = 1;
                while x + 4 <= w - 1 {
                    let c = vld1q_f32(mid.add(x));
                    let l = vsubq_f32(vsubq_f32(vsubq_f32(vsubq_f32(vmulq_f32(four, c), vld1q_f32(mid.add(x - 1))), vld1q_f32(mid.add(x + 1))), vld1q_f32(up.add(x))), vld1q_f32(dn.add(x)));
                    vs = vaddq_f32(vs, l);
                    vs2 = vfmaq_f32(vs2, l, l);
                    vm = vaddq_f32(vm, c);
                    x += 4;
                }
                let (mut rs, mut rs2, mut rm) = (vaddvq_f32(vs), vaddvq_f32(vs2), vaddvq_f32(vm));
                while x < w - 1 {
                    let l = 4.0 * *mid.add(x) - *mid.add(x - 1) - *mid.add(x + 1) - *up.add(x) - *dn.add(x);
                    rs += l;
                    rs2 += l * l;
                    rm += *mid.add(x);
                    x += 1;
                }
                s += rs as f64;
                s2 += rs2 as f64;
                sm += rm as f64;
                n += w - 2;
            }
        }
        let nf = n as f64;
        let var = s2 / nf - (s / nf) * (s / nf);
        let mean = sm / nf;
        var / (mean * mean)
    }

    /// V2 of the shifted accumulate: a constant sub-pixel shift is a fixed 2x2 filter of contiguous loads, so whole rows vectorise.
    pub fn accumulate_shifted(acc: &mut [f32], src: &[f32], w: usize, h: usize, dx: f64, dy: f64) {
        let (ix, iy) = (dx.floor(), dy.floor());
        let (fx, fy) = ((dx - ix) as f32, (dy - iy) as f32);
        let (ix, iy) = (ix as isize, iy as isize);
        let (w00, w10, w01, w11) = ((1.0 - fx) * (1.0 - fy), fx * (1.0 - fy), (1.0 - fx) * fy, fx * fy);
        let at = |x: isize, y: isize| src[(y.clamp(0, h as isize - 1) as usize) * w + x.clamp(0, w as isize - 1) as usize];
        // columns where x + ix and x + ix + 1 are both inside the row
        let x_lo = (-ix).max(0) as usize;
        let x_hi = ((w as isize - 2 - ix).min(w as isize - 1)).max(0) as usize; // exclusive bound for the vector loop below
        for y in 0..h as isize {
            let (sy0, sy1) = (y + iy, y + iy + 1);
            let row = &mut acc[y as usize * w..(y as usize + 1) * w];
            let scalar = |x: usize, row: &mut [f32]| {
                let (sx, sy) = (x as isize + ix, y + iy);
                row[x] += at(sx, sy) * w00 + at(sx + 1, sy) * w10 + at(sx, sy + 1) * w01 + at(sx + 1, sy + 1) * w11;
            };
            if sy0 < 0 || sy1 >= h as isize || x_hi <= x_lo + 8 {
                for x in 0..w { scalar(x, row); }
                continue;
            }
            for x in 0..x_lo.min(w) { scalar(x, row); }
            let mut x = x_lo;
            unsafe {
                let (v00, v10, v01, v11) = (vdupq_n_f32(w00), vdupq_n_f32(w10), vdupq_n_f32(w01), vdupq_n_f32(w11));
                let (r0, r1) = (src.as_ptr().add(sy0 as usize * w), src.as_ptr().add(sy1 as usize * w));
                while x + 4 <= x_hi {
                    let sx = (x as isize + ix) as usize;
                    let mut v = vmulq_f32(vld1q_f32(r0.add(sx)), v00);
                    v = vfmaq_f32(v, vld1q_f32(r0.add(sx + 1)), v10);
                    v = vfmaq_f32(v, vld1q_f32(r1.add(sx)), v01);
                    v = vfmaq_f32(v, vld1q_f32(r1.add(sx + 1)), v11);
                    vst1q_f32(row.as_mut_ptr().add(x), vaddq_f32(vld1q_f32(row.as_ptr().add(x)), v));
                    x += 4;
                }
            }
            for x in x..w { scalar(x, row); }
        }
    }
}

fn time<T>(label: &str, n: usize, mut f: impl FnMut() -> T) -> f64 {
    let _ = f(); // warm up
    let t = Instant::now();
    for _ in 0..n { std::hint::black_box(f()); }
    let ms = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
    println!("  {label:46} {ms:8.2} ms / frame");
    ms
}

fn main() {
    let raw = frame(1);
    let flat: Vec<f32> = (0..W * H).map(|i| 0.7 + 0.5 * (((i % W) as f32 - 972.0).abs() / 972.0).min(1.0) * 0.0 + 0.3 * ((i / W) as f32 / H as f32)).collect();
    let inv_flat: Vec<f32> = flat.iter().map(|f| 1.0 / f).collect();
    let cal = Calibration { width: W, height: H, dark: None, flat: Some(Arc::new(flat.clone())), inv_flat: Some(Arc::new(inv_flat.clone())) };
    let valid = vec![true; (W / 2) * (H / 2)];
    let n = 10;
    println!("pass A per frame (calibrate flat, 2x2 bin, sharpness), {W}x{H} u16:");
    let t0 = time("V0 current pipeline code", n, || pass_a_current(&raw, &cal, &valid));
    let t1 = time("V1 fused scalar, written to auto-vectorise", n, || pass_a_fused_scalar(&raw, &inv_flat));
    let (a, sa) = pass_a_current(&raw, &cal, &valid);
    let (b, sb) = pass_a_fused_scalar(&raw, &inv_flat);
    let maxd = a.d_or(&a).iter().zip(&b).map(|(x, y)| (x - y).abs() / x.abs().max(1.0)).fold(0.0f32, f32::max);
    println!("    check: max relative difference of the binned image {maxd:.2e}, sharpness {sa:.6e} vs {sb:.6e}");
    #[cfg(target_arch = "aarch64")]
    {
        let t2 = time("V2 explicit NEON", n, || neon::pass_a(&raw, &inv_flat));
        let (c, sc) = neon::pass_a(&raw, &inv_flat);
        let maxd = a.iter().zip(&c).map(|(x, y)| (x - y).abs() / x.abs().max(1.0)).fold(0.0f32, f32::max);
        println!("    check: max relative difference {maxd:.2e}, sharpness {sc:.6e}; speed-up over V0 {:.1}x, over V1 {:.1}x", t0 / t2, t1 / t2);
    }
    println!("\nshifted accumulate (bilinear, constant shift) of one {W}x{H} f32 frame:");
    let src: Vec<f32> = (0..W * H).map(|i| (i % 4096) as f32).collect();
    let mut acc0 = vec![0f32; W * H];
    let a0 = time("V0 current accumulate_shifted", n, || accumulate_shifted(&mut acc0, &src, W, H, 2.3, -1.6));
    #[cfg(target_arch = "aarch64")]
    {
        let mut acc2 = vec![0f32; W * H];
        let a2 = time("V2 explicit NEON", n, || neon::accumulate_shifted(&mut acc2, &src, W, H, 2.3, -1.6));
        let (mut r0, mut r2) = (vec![0f32; W * H], vec![0f32; W * H]);
        accumulate_shifted(&mut r0, &src, W, H, 2.3, -1.6);
        neon::accumulate_shifted(&mut r2, &src, W, H, 2.3, -1.6);
        let maxd = r0.iter().zip(&r2).map(|(x, y)| (x - y).abs()).fold(0.0f32, f32::max);
        println!("    check: max absolute difference {maxd:.2e}; speed-up {:.1}x", a0 / a2);
    }
    let _ = (a0, t0);
}

trait DOr { fn d_or(&self, o: &Self) -> &Vec<f32>; }
impl DOr for Vec<f32> { fn d_or(&self, _o: &Self) -> &Vec<f32> { self } }
