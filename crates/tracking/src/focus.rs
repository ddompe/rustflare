//! Focus assist: a sharpness score for a frame and a seeing-robust ("lucky") score for a burst of frames.
//!
//! The score is the root-mean-square gradient of the bright part of a window around the Sun divided by the mean
//! brightness there, so it does not change with exposure or gain, and it rises as the limb and the surface detail get
//! crisper. Seeing makes single frames jump around, so a burst is summarised by the mean of its sharpest quarter.

use rustflare_core::{Frame, PixelFormat};

/// Half-size of the square window the score is computed on (pixels). 640 x 640 is a millisecond or two on a Pi.
pub const WINDOW_HALF: i64 = 320;

fn value(f: &Frame, x: usize, y: usize) -> f64 {
    let i = y * f.width as usize + x;
    match f.format {
        PixelFormat::Mono8 => f.data[i] as f64,
        PixelFormat::Mono16 => u16::from_le_bytes([f.data[2 * i], f.data[2 * i + 1]]) as f64,
    }
}

/// Brightness of a 2x2 block for a Bayer frame (all colours summed, which removes the mosaic pattern), or of a pixel.
fn lum(f: &Frame, x: usize, y: usize) -> f64 {
    if f.bayer.is_some() {
        let (x, y) = (x * 2, y * 2);
        value(f, x, y) + value(f, x + 1, y) + value(f, x, y + 1) + value(f, x + 1, y + 1)
    } else {
        value(f, x, y)
    }
}

/// Sharpness of the window of `frame` centred on `(cx, cy)` (sensor pixels). `None` when there is nothing bright
/// enough to judge (cap on, Sun out of view).
pub fn sharpness(frame: &Frame, cx: f64, cy: f64) -> Option<f64> {
    let scale = if frame.bayer.is_some() { 2 } else { 1 };
    let (w, h) = ((frame.width as i64) / scale, (frame.height as i64) / scale);
    let (cx, cy) = ((cx as i64) / scale, (cy as i64) / scale);
    let half = WINDOW_HALF / scale;
    let x0 = (cx - half).clamp(0, w - 2);
    let y0 = (cy - half).clamp(0, h - 2);
    let x1 = (cx + half).clamp(x0 + 2, w - 1);
    let y1 = (cy + half).clamp(y0 + 2, h - 1);
    if x1 - x0 < 8 || y1 - y0 < 8 {
        return None;
    }
    // brightest value in the window, to ignore the dark sky around the Sun
    let mut max = 0.0f64;
    let step = 4;
    let mut y = y0;
    while y < y1 {
        let mut x = x0;
        while x < x1 {
            max = max.max(lum(frame, x as usize, y as usize));
            x += step;
        }
        y += step;
    }
    if max <= 0.0 {
        return None;
    }
    let thresh = max * 0.2;
    let (mut sum_g2, mut sum_p, mut n) = (0.0f64, 0.0f64, 0u64);
    for y in y0..y1 {
        for x in x0..x1 {
            let p = lum(frame, x as usize, y as usize);
            if p < thresh {
                continue;
            }
            let gx = lum(frame, (x + 1) as usize, y as usize) - p;
            let gy = lum(frame, x as usize, (y + 1) as usize) - p;
            sum_g2 += gx * gx + gy * gy;
            sum_p += p;
            n += 1;
        }
    }
    if n < 500 || sum_p <= 0.0 {
        return None;
    }
    let mean_p = sum_p / n as f64;
    Some((sum_g2 / n as f64).sqrt() / mean_p * 100.0)
}

/// The "lucky" score of a burst: the mean of the sharpest quarter of the frame scores (at least one).
pub fn lucky_score(scores: &[f64]) -> Option<f64> {
    if scores.is_empty() {
        return None;
    }
    let mut s = scores.to_vec();
    s.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let k = (s.len() / 4).max(1);
    Some(s[..k].iter().sum::<f64>() / k as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bright disc of radius `r` with an edge blurred over `blur` pixels, plus some surface texture.
    fn disc(blur: f64, level: f64) -> Frame {
        let (w, h) = (800usize, 800usize);
        let mut d = Vec::with_capacity(w * h * 2);
        for y in 0..h {
            for x in 0..w {
                let (dx, dy) = (x as f64 - 400.0, y as f64 - 400.0);
                let r = (dx * dx + dy * dy).sqrt();
                let edge = 1.0 / (1.0 + ((r - 250.0) / blur.max(0.3)).exp());
                let tex = 1.0 + 0.1 * ((x as f64 / 9.0).sin() * (y as f64 / 7.0).cos());
                let v = (edge * tex * level * 40000.0) as u16 + 200;
                d.extend_from_slice(&v.to_le_bytes());
            }
        }
        Frame::new(w as u32, h as u32, PixelFormat::Mono16, d)
    }

    #[test]
    fn a_sharper_edge_scores_higher() {
        let (sharp, soft, soft2) = (sharpness(&disc(1.0, 1.0), 400.0, 400.0).unwrap(), sharpness(&disc(4.0, 1.0), 400.0, 400.0).unwrap(), sharpness(&disc(9.0, 1.0), 400.0, 400.0).unwrap());
        assert!(sharp > soft && soft > soft2, "{sharp} {soft} {soft2}");
    }

    #[test]
    fn the_score_does_not_depend_on_exposure() {
        let a = sharpness(&disc(2.0, 1.0), 400.0, 400.0).unwrap();
        let b = sharpness(&disc(2.0, 0.5), 400.0, 400.0).unwrap();
        assert!((a - b).abs() / a < 0.03, "{a} vs {b}");
    }

    #[test]
    fn a_dark_frame_has_no_score() {
        let f = Frame::new(800, 800, PixelFormat::Mono16, vec![0; 800 * 800 * 2]);
        assert_eq!(sharpness(&f, 400.0, 400.0), None);
    }

    #[test]
    fn lucky_score_is_the_mean_of_the_best_quarter() {
        let s = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        assert_eq!(lucky_score(&s), Some(7.5));
        assert_eq!(lucky_score(&[3.0]), Some(3.0));
        assert_eq!(lucky_score(&[]), None);
    }
}
