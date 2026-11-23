//! The stacker's per-pixel inner loops, vectorised. On aarch64 (Raspberry Pi 5, Apple silicon) they use NEON intrinsics; elsewhere a
//! portable version written so the compiler can vectorise it. Both give the same results (see the tests); the NEON ones were measured
//! 3.9x to 6.8x faster than the straightforward scalar code on a Pi 5 (docs/project/stacker-simd-gpu-spike-2026-11-22.md).

/// Calibrate (dark off, times 1/flat) and bin 2x2 a raw frame in one pass: `out` is `w/2 x h/2`. `w` must be a multiple of 8 for the
/// NEON path (anything else takes the portable path).
pub fn bin2_calibrated(raw: &[u16], dark: Option<&[f32]>, inv_flat: Option<&[f32]>, w: usize, h: usize, out: &mut [f32]) {
    #[cfg(target_arch = "aarch64")]
    if w % 8 == 0 {
        neon::bin2_calibrated(raw, dark, inv_flat, w, h, out);
        return;
    }
    portable::bin2_calibrated(raw, dark, inv_flat, w, h, out);
}

/// Sums over the interior pixels weighted by `mask` (1 = counted): (sum of Laplacian, sum of its square, sum of brightness, count).
pub fn lap_stats(img: &[f32], mask: &[f32], w: usize, h: usize) -> (f64, f64, f64, f64) {
    #[cfg(target_arch = "aarch64")]
    {
        return neon::lap_stats(img, mask, w, h);
    }
    #[cfg(not(target_arch = "aarch64"))]
    portable::lap_stats(img, mask, w, h)
}

/// `acc += src` displaced by (`dx`, `dy`) with bilinear interpolation; edges clamp.
pub fn accumulate_shifted(acc: &mut [f32], src: &[f32], w: usize, h: usize, dx: f64, dy: f64) {
    #[cfg(target_arch = "aarch64")]
    {
        neon::accumulate_shifted(acc, src, w, h, dx, dy);
        return;
    }
    #[cfg(not(target_arch = "aarch64"))]
    portable::accumulate_shifted(acc, src, w, h, dx, dy)
}

/// The Laplacian of `img` (zero on the border and wherever `mask` is 0): `out = mask * (4c - left - right - up - down)`.
pub fn laplacian_masked(img: &[f32], mask: &[f32], w: usize, h: usize) -> Vec<f32> {
    let mut out = vec![0f32; w * h];
    for y in 1..h - 1 {
        let (up, mid, dn, mk) = (&img[(y - 1) * w..y * w], &img[y * w..(y + 1) * w], &img[(y + 1) * w..(y + 2) * w], &mask[y * w..(y + 1) * w]);
        let o = &mut out[y * w..(y + 1) * w];
        for x in 1..w - 1 {
            o[x] = (4.0 * mid[x] - mid[x - 1] - mid[x + 1] - up[x] - dn[x]) * mk[x];
        }
    }
    out
}

/// Sum of `a * b * mask` and of `a * mask` over whole slices, in f32 chunks (vectorises) accumulated in f64.
pub fn masked_dot_and_sum(a: &[f32], b: &[f32], mask: &[f32]) -> (f64, f64) {
    let (mut dot, mut sum) = (0.0f64, 0.0f64);
    for ((ca, cb), cm) in a.chunks(4096).zip(b.chunks(4096)).zip(mask.chunks(4096)) {
        let (mut d, mut s) = (0.0f32, 0.0f32);
        for ((x, y), m) in ca.iter().zip(cb).zip(cm) {
            d += x * y * m;
            s += x * m;
        }
        dot += d as f64;
        sum += s as f64;
    }
    (dot, sum)
}

pub mod portable {
    pub fn bin2_calibrated(raw: &[u16], dark: Option<&[f32]>, inv_flat: Option<&[f32]>, w: usize, h: usize, out: &mut [f32]) {
        let ow = w / 2;
        let px = |i: usize| {
            let mut v = raw[i] as f32;
            if let Some(d) = dark { v = (v - d[i]).max(0.0); }
            if let Some(f) = inv_flat { v *= f[i]; }
            v
        };
        for y in 0..h / 2 {
            for x in 0..ow {
                let i = 2 * y * w + 2 * x;
                out[y * ow + x] = 0.25 * (px(i) + px(i + 1) + px(i + w) + px(i + w + 1));
            }
        }
    }

    pub fn lap_stats(img: &[f32], mask: &[f32], w: usize, h: usize) -> (f64, f64, f64, f64) {
        let (mut s, mut s2, mut sm, mut n) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        for y in 1..h - 1 {
            let (up, mid, dn, mk) = (&img[(y - 1) * w..y * w], &img[y * w..(y + 1) * w], &img[(y + 1) * w..(y + 2) * w], &mask[y * w..(y + 1) * w]);
            let (mut rs, mut rs2, mut rm, mut rn) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
            for x in 1..w - 1 {
                let m = mk[x];
                let l = (4.0 * mid[x] - mid[x - 1] - mid[x + 1] - up[x] - dn[x]) * m;
                rs += l;
                rs2 += l * l / if m > 0.0 { m } else { 1.0 };
                rm += mid[x] * m;
                rn += m;
            }
            s += rs as f64;
            s2 += rs2 as f64;
            sm += rm as f64;
            n += rn as f64;
        }
        (s, s2, sm, n)
    }

    pub fn accumulate_shifted(acc: &mut [f32], src: &[f32], w: usize, h: usize, dx: f64, dy: f64) {
        let (ix, iy) = (dx.floor(), dy.floor());
        let (fx, fy) = ((dx - ix) as f32, (dy - iy) as f32);
        let (ix, iy) = (ix as isize, iy as isize);
        let at = |x: isize, y: isize| src[(y.clamp(0, h as isize - 1) as usize) * w + x.clamp(0, w as isize - 1) as usize];
        for y in 0..h as isize {
            for x in 0..w as isize {
                let (sx, sy) = (x + ix, y + iy);
                acc[y as usize * w + x as usize] += at(sx, sy) * (1.0 - fx) * (1.0 - fy) + at(sx + 1, sy) * fx * (1.0 - fy) + at(sx, sy + 1) * (1.0 - fx) * fy + at(sx + 1, sy + 1) * fx * fy;
            }
        }
    }
}

#[cfg(target_arch = "aarch64")]
pub mod neon {
    use std::arch::aarch64::*;

    /// Eight pixels of two rows per step: widen to f32, take the dark off, multiply by 1/flat, add the rows, add column pairs.
    pub fn bin2_calibrated(raw: &[u16], dark: Option<&[f32]>, inv_flat: Option<&[f32]>, w: usize, h: usize, out: &mut [f32]) {
        let ow = w / 2;
        assert!(raw.len() >= w * h && out.len() >= ow * (h / 2) && dark.map_or(true, |d| d.len() >= w * h) && inv_flat.map_or(true, |f| f.len() >= w * h));
        unsafe {
            let (q, zero) = (vdupq_n_f32(0.25), vdupq_n_f32(0.0));
            for y in 0..h / 2 {
                let rows = [2 * y, 2 * y + 1];
                let o = out.as_mut_ptr().add(y * ow);
                let mut x = 0;
                while x + 8 <= w {
                    let mut parts = [vdupq_n_f32(0.0); 4]; // row0 lo, row0 hi, row1 lo, row1 hi
                    for (r, &row) in rows.iter().enumerate() {
                        let base = row * w + x;
                        let v = vld1q_u16(raw.as_ptr().add(base));
                        let (mut lo, mut hi) = (vcvtq_f32_u32(vmovl_u16(vget_low_u16(v))), vcvtq_f32_u32(vmovl_high_u16(v)));
                        if let Some(d) = dark {
                            lo = vmaxq_f32(vsubq_f32(lo, vld1q_f32(d.as_ptr().add(base))), zero);
                            hi = vmaxq_f32(vsubq_f32(hi, vld1q_f32(d.as_ptr().add(base + 4))), zero);
                        }
                        if let Some(f) = inv_flat {
                            lo = vmulq_f32(lo, vld1q_f32(f.as_ptr().add(base)));
                            hi = vmulq_f32(hi, vld1q_f32(f.as_ptr().add(base + 4)));
                        }
                        parts[2 * r] = lo;
                        parts[2 * r + 1] = hi;
                    }
                    let sum = vpaddq_f32(vaddq_f32(parts[0], parts[2]), vaddq_f32(parts[1], parts[3]));
                    vst1q_f32(o.add(x / 2), vmulq_f32(sum, q));
                    x += 8;
                }
            }
        }
    }

    pub fn lap_stats(img: &[f32], mask: &[f32], w: usize, h: usize) -> (f64, f64, f64, f64) {
        let (mut s, mut s2, mut sm, mut n) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        assert!(img.len() >= w * h && mask.len() >= w * h);
        unsafe {
            let four = vdupq_n_f32(4.0);
            for y in 1..h - 1 {
                let (up, mid, dn, mk) = (img.as_ptr().add((y - 1) * w), img.as_ptr().add(y * w), img.as_ptr().add((y + 1) * w), mask.as_ptr().add(y * w));
                let (mut vs, mut vs2, mut vm, mut vn) = (vdupq_n_f32(0.0), vdupq_n_f32(0.0), vdupq_n_f32(0.0), vdupq_n_f32(0.0));
                let mut x = 1;
                while x + 4 <= w - 1 {
                    let (c, m) = (vld1q_f32(mid.add(x)), vld1q_f32(mk.add(x)));
                    let l = vsubq_f32(vsubq_f32(vsubq_f32(vsubq_f32(vmulq_f32(four, c), vld1q_f32(mid.add(x - 1))), vld1q_f32(mid.add(x + 1))), vld1q_f32(up.add(x))), vld1q_f32(dn.add(x)));
                    let lm = vmulq_f32(l, m); // the mask is 0 or 1
                    vs = vaddq_f32(vs, lm);
                    vs2 = vfmaq_f32(vs2, lm, l);
                    vm = vfmaq_f32(vm, c, m);
                    vn = vaddq_f32(vn, m);
                    x += 4;
                }
                let (mut rs, mut rs2, mut rm, mut rn) = (vaddvq_f32(vs), vaddvq_f32(vs2), vaddvq_f32(vm), vaddvq_f32(vn));
                while x < w - 1 {
                    let m = *mk.add(x);
                    let l = 4.0 * *mid.add(x) - *mid.add(x - 1) - *mid.add(x + 1) - *up.add(x) - *dn.add(x);
                    rs += l * m;
                    rs2 += l * l * m;
                    rm += *mid.add(x) * m;
                    rn += m;
                    x += 1;
                }
                s += rs as f64;
                s2 += rs2 as f64;
                sm += rm as f64;
                n += rn as f64;
            }
        }
        (s, s2, sm, n)
    }

    /// A constant sub-pixel shift is a fixed 2x2 filter of contiguous loads, so whole rows vectorise; the edges take the scalar path.
    pub fn accumulate_shifted(acc: &mut [f32], src: &[f32], w: usize, h: usize, dx: f64, dy: f64) {
        let (ix, iy) = (dx.floor(), dy.floor());
        let (fx, fy) = ((dx - ix) as f32, (dy - iy) as f32);
        let (ix, iy) = (ix as isize, iy as isize);
        let (w00, w10, w01, w11) = ((1.0 - fx) * (1.0 - fy), fx * (1.0 - fy), (1.0 - fx) * fy, fx * fy);
        let at = |x: isize, y: isize| src[(y.clamp(0, h as isize - 1) as usize) * w + x.clamp(0, w as isize - 1) as usize];
        // columns whose source column and the next are both inside the row: x in [x_lo, x_hi)
        let x_lo = (-ix).max(0) as usize;
        let x_hi = (w as isize - 1 - ix).clamp(0, w as isize) as usize;
        for y in 0..h as isize {
            let (sy0, sy1) = (y + iy, y + iy + 1);
            let row = &mut acc[y as usize * w..(y as usize + 1) * w];
            let scalar = |x: usize, row: &mut [f32]| {
                let (sx, sy) = (x as isize + ix, y + iy);
                row[x] += at(sx, sy) * w00 + at(sx + 1, sy) * w10 + at(sx, sy + 1) * w01 + at(sx + 1, sy + 1) * w11;
            };
            if sy0 < 0 || sy1 >= h as isize || x_hi < x_lo + 8 {
                for x in 0..w {
                    scalar(x, row);
                }
                continue;
            }
            for x in 0..x_lo {
                scalar(x, row);
            }
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
            for x in x..w {
                scalar(x, row);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lcg(seed: &mut u32) -> u32 { *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223); *seed >> 8 }

    #[test]
    fn the_fast_kernels_agree_with_the_portable_ones() {
        let (w, h) = (96usize, 40usize);
        let mut s = 7u32;
        let raw: Vec<u16> = (0..w * h).map(|_| (lcg(&mut s) % 60000) as u16).collect();
        let dark: Vec<f32> = (0..w * h).map(|_| (lcg(&mut s) % 300) as f32).collect();
        let inv: Vec<f32> = (0..w * h).map(|_| 1.0 / (0.5 + (lcg(&mut s) % 100) as f32 / 100.0)).collect();
        for (d, f) in [(None, None), (Some(&dark[..]), None), (None, Some(&inv[..])), (Some(&dark[..]), Some(&inv[..]))] {
            let (mut a, mut b) = (vec![0f32; w / 2 * h / 2], vec![0f32; w / 2 * h / 2]);
            bin2_calibrated(&raw, d, f, w, h, &mut a);
            portable::bin2_calibrated(&raw, d, f, w, h, &mut b);
            let worst = a.iter().zip(&b).map(|(x, y)| (x - y).abs() / y.abs().max(1.0)).fold(0.0f32, f32::max);
            assert!(worst < 1e-5, "bin2 differs by {worst} (dark {}, flat {})", d.is_some(), f.is_some());
        }
        // Laplacian sums with a mask that has holes
        let img: Vec<f32> = (0..w * h).map(|_| (lcg(&mut s) % 5000) as f32).collect();
        let mask: Vec<f32> = (0..w * h).map(|i| if (i % w) < 20 || (i / w) % 7 == 0 { 0.0 } else { 1.0 }).collect();
        let (a, b) = (lap_stats(&img, &mask, w, h), portable::lap_stats(&img, &mask, w, h));
        for (x, y) in [(a.0, b.0), (a.1, b.1), (a.2, b.2), (a.3, b.3)] {
            assert!((x - y).abs() <= 1e-4 * y.abs().max(1.0), "{a:?} vs {b:?}");
        }
        // shifted accumulate, several shifts including ones that push whole rows outside the frame
        let src: Vec<f32> = (0..w * h).map(|_| (lcg(&mut s) % 1000) as f32).collect();
        for (dx, dy) in [(0.0, 0.0), (2.3, -1.6), (-5.75, 3.2), (40.5, 0.5), (-60.0, -39.0), (0.0, 7.9)] {
            let (mut a, mut b) = (vec![1.0f32; w * h], vec![1.0f32; w * h]);
            accumulate_shifted(&mut a, &src, w, h, dx, dy);
            portable::accumulate_shifted(&mut b, &src, w, h, dx, dy);
            let worst = a.iter().zip(&b).map(|(x, y)| (x - y).abs()).fold(0.0f32, f32::max);
            assert!(worst < 2e-3, "accumulate differs by {worst} at shift ({dx}, {dy})");
        }
    }
}
