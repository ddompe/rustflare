//! Small image helpers: binning, a validity mask, and the sharpness score used to rank frames.

/// A single-channel f32 image, row-major.
#[derive(Clone, Debug)]
pub struct Img {
    pub w: usize,
    pub h: usize,
    pub d: Vec<f32>,
}

impl Img {
    pub fn new(w: usize, h: usize) -> Img { Img { w, h, d: vec![0.0; w * h] } }
}

/// Average of `n` by `n` blocks (edges that do not fill a block are dropped).
pub fn bin(src: &[f32], w: usize, h: usize, n: usize) -> Img {
    if n <= 1 {
        return Img { w, h, d: src.to_vec() };
    }
    let (ow, oh) = (w / n, h / n);
    let mut out = Img::new(ow, oh);
    let inv = 1.0 / (n * n) as f32;
    for y in 0..oh {
        for x in 0..ow {
            let mut s = 0.0;
            for dy in 0..n {
                let row = (y * n + dy) * w + x * n;
                for dx in 0..n {
                    s += src[row + dx];
                }
            }
            out.d[y * ow + x] = s * inv;
        }
    }
    out
}

/// Pixels that carry signal: brighter than `floor` in the reference image, shrunk by `erode` pixels so that the sharp edge of a
/// black coronagraph mask (or of the frame) stays out of the sharpness score and the alignment.
pub fn valid_mask(reference: &Img, floor: f32, erode: usize) -> Vec<bool> {
    let (w, h) = (reference.w, reference.h);
    let on: Vec<bool> = reference.d.iter().map(|&v| v > floor).collect();
    // separable erosion by a (2*erode+1) square: first along rows, then along columns
    let mut tmp = vec![false; w * h];
    for y in 0..h {
        let row = &on[y * w..(y + 1) * w];
        let mut run = 0usize; // consecutive true pixels ending here
        let mut left = vec![0usize; w];
        for x in 0..w {
            run = if row[x] { run + 1 } else { 0 };
            left[x] = run;
        }
        let mut run = 0usize;
        for x in (0..w).rev() {
            run = if row[x] { run + 1 } else { 0 };
            tmp[y * w + x] = left[x] > erode && run > erode;
        }
    }
    let mut out = vec![false; w * h];
    for x in 0..w {
        let mut run = 0usize;
        let mut up = vec![0usize; h];
        for y in 0..h {
            run = if tmp[y * w + x] { run + 1 } else { 0 };
            up[y] = run;
        }
        let mut run = 0usize;
        for y in (0..h).rev() {
            run = if tmp[y * w + x] { run + 1 } else { 0 };
            out[y * w + x] = up[y] > erode && run > erode;
        }
    }
    out
}

/// Sharpness of an image: the variance of the Laplacian over the valid pixels, divided by the squared mean brightness so
/// frames of a burst stay comparable when the light changes. Higher is sharper.
pub fn sharpness(img: &Img, valid: &[bool]) -> f64 { sharpness_masked(img, &mask_f32(valid)) }

/// 1.0 where `valid`, else 0.0: the form the vectorised sharpness wants, built once for a whole recording.
pub fn mask_f32(valid: &[bool]) -> Vec<f32> { valid.iter().map(|&v| if v { 1.0 } else { 0.0 }).collect() }

/// `sharpness` with a precomputed 0/1 mask.
pub fn sharpness_masked(img: &Img, mask: &[f32]) -> f64 {
    if img.w < 3 || img.h < 3 {
        return 0.0;
    }
    let (s, s2, sm, n) = crate::simd::lap_stats(&img.d, mask, img.w, img.h);
    if n <= 0.0 {
        return 0.0;
    }
    let var = s2 / n - (s / n) * (s / n);
    let mean = sm / n;
    if mean <= 0.0 { 0.0 } else { var / (mean * mean) }
}

/// Box blur of radius `r` (edges clamp), separable running sums.
pub fn box_blur(src: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let mut tmp = vec![0f32; w * h];
    let n = (2 * r + 1) as f32;
    for y in 0..h {
        let row = &src[y * w..(y + 1) * w];
        let at = |x: isize| row[x.clamp(0, w as isize - 1) as usize];
        let mut s: f32 = (-(r as isize)..=r as isize).map(at).sum();
        for x in 0..w {
            tmp[y * w + x] = s / n;
            s += at(x as isize + r as isize + 1) - at(x as isize - r as isize);
        }
    }
    let mut out = vec![0f32; w * h];
    for x in 0..w {
        let at = |y: isize| tmp[y.clamp(0, h as isize - 1) as usize * w + x];
        let mut s: f32 = (-(r as isize)..=r as isize).map(at).sum();
        for y in 0..h {
            out[y * w + x] = s / n;
            s += at(y as isize + r as isize + 1) - at(y as isize - r as isize);
        }
    }
    out
}
