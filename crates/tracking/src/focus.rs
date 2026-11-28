//! Focus assist: a sharpness score for a frame and a seeing-robust ("lucky") score, with its uncertainty, for a burst of frames.
//!
//! The score is the root-mean-square gradient of the Sun's *interior* (the surface texture) divided by the mean brightness
//! there, so it does not change with exposure or gain, and it rises as the detail gets crisper. The limb is left out: turning a
//! focuser shifts the Sun a little and the limb moves through the window, which the score then mistook for focus. The window is
//! fixed in the frame when the session starts (it does not follow the Sun) and only the part of it that lies well inside the
//! Sun's disc (`INTERIOR` of the radius, measured from the Sun's current position) counts. Seeing makes single frames jump
//! around, so a burst is summarised by the mean of its sharpest frames, with the scatter of those frames as an error estimate.

use rustflare_core::{Frame, PixelFormat};

/// Largest half-size of the square scoring window (pixels). 640 x 640 is a millisecond or two on a Pi.
pub const WINDOW_HALF: i64 = 320;
/// Only pixels closer than this fraction of the Sun's radius to its centre are scored.
pub const INTERIOR: f64 = 0.8;
/// Fraction of a burst's frames that count (the sharpest), and the least number of them.
pub const LUCKY_FRACTION: f64 = 0.125;
pub const LUCKY_MIN: usize = 3;

/// The scoring window: a centre and a half-size, in sensor pixels, fixed for a focus session.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Window {
    pub cx: f64,
    pub cy: f64,
    pub half: i64,
}

impl Window {
    /// A window inside the disc at `(x, y)` with radius `r`, or a frame-centred one when the Sun is unknown.
    pub fn new(sun: Option<(f64, f64, f64)>, frame_w: u32, frame_h: u32) -> Window {
        match sun {
            Some((x, y, r)) => Window { cx: x, cy: y, half: ((r * 0.6) as i64).clamp(24, WINDOW_HALF) },
            None => Window { cx: frame_w as f64 / 2.0, cy: frame_h as f64 / 2.0, half: WINDOW_HALF },
        }
    }
}

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

/// Sharpness of the surface texture in `win`, counting only pixels inside `INTERIOR` of the Sun's disc `sun = (x, y, radius)`
/// (the whole window, limb included, when the Sun is unknown). `None` when there is nothing bright enough to judge (cap on,
/// Sun out of view) or too little of the window lies on the disc.
pub fn sharpness(frame: &Frame, win: Window, sun: Option<(f64, f64, f64)>) -> Option<f64> {
    let scale = if frame.bayer.is_some() { 2 } else { 1 };
    let (w, h) = ((frame.width as i64) / scale, (frame.height as i64) / scale);
    let (cx, cy) = ((win.cx as i64) / scale, (win.cy as i64) / scale);
    let half = win.half / scale;
    let x0 = (cx - half).clamp(0, w - 2);
    let y0 = (cy - half).clamp(0, h - 2);
    let x1 = (cx + half).clamp(x0 + 2, w - 1);
    let y1 = (cy + half).clamp(y0 + 2, h - 1);
    if x1 - x0 < 8 || y1 - y0 < 8 {
        return None;
    }
    let inside = |x: i64, y: i64| match sun {
        Some((sx, sy, r)) => {
            let (dx, dy) = ((x * scale) as f64 - sx, (y * scale) as f64 - sy);
            (dx * dx + dy * dy).sqrt() <= INTERIOR * r
        }
        None => true,
    };
    // brightest value in the window, to ignore the dark sky around the Sun
    let mut max = 0.0f64;
    let step = 4;
    let mut y = y0;
    while y < y1 {
        let mut x = x0;
        while x < x1 {
            if inside(x, y) {
                max = max.max(lum(frame, x as usize, y as usize));
            }
            x += step;
        }
        y += step;
    }
    if max <= 0.0 {
        return None;
    }
    let thresh = max * 0.2;
    let (mut sum_g2, mut sum_p, mut n) = (0.0f64, 0.0f64, 0u64);
    // pixel noise adds a gradient that has nothing to do with focus (and dominates where the surface is smooth): estimate its
    // level from the response of a 3x3 mask that cancels smooth detail (Immerkaer), on every third pixel, and take it off
    let (mut nz_sum, mut nz_n) = (0.0f64, 0u64);
    for y in y0..y1 {
        for x in x0..x1 {
            if !inside(x, y) {
                continue;
            }
            let p = lum(frame, x as usize, y as usize);
            if p < thresh {
                continue;
            }
            if x % 3 == 1 && y % 3 == 1 && x > x0 && y > y0 && x < x1 - 1 && y < y1 - 1 {
                let l = |dx: i64, dy: i64| lum(frame, (x + dx) as usize, (y + dy) as usize);
                let r = l(-1, -1) + l(1, -1) + l(-1, 1) + l(1, 1) - 2.0 * (l(0, -1) + l(-1, 0) + l(1, 0) + l(0, 1)) + 4.0 * p;
                nz_sum += r.abs();
                nz_n += 1;
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
    // white noise of standard deviation s gives mean |mask response| = 6 s sqrt(2 / pi); its gradient energy is 4 s^2 per pixel
    let sigma = if nz_n > 0 { nz_sum / nz_n as f64 / 6.0 * (std::f64::consts::PI / 2.0).sqrt() } else { 0.0 };
    let energy = (sum_g2 / n as f64 - 4.0 * sigma * sigma).max(0.0);
    Some(energy.sqrt() / mean_p * 100.0)
}

/// Where a planet is in a frame, in the (half-resolution, for a Bayer frame) luminance pixels of [`lum`], and how far above the sky it stands.
#[derive(Clone, Copy, Debug, PartialEq)]
struct PlanetBlob {
    cx: f64,
    cy: f64,
    half_w: f64,
    half_h: f64,
    /// Sky level and the planet's brightness above it.
    sky: f64,
    level: f64,
}

/// Finds the brightest compact object in a frame without help from the tracker: 4 x 4 bins, the sky level from the median bin, the
/// blob of bins above a quarter of the way up that holds the brightest bin (8-connected, so a ring and its globe stay together).
/// `None` when nothing stands clearly above the sky, or the blob is a few bins or most of the frame (cloud, a covered telescope).
fn find_planet(frame: &Frame) -> Option<PlanetBlob> {
    const BIN: usize = 4;
    let scale = if frame.bayer.is_some() { 2 } else { 1 };
    let (w, h) = (frame.width as usize / scale, frame.height as usize / scale);
    let (bw, bh) = (w / BIN, h / BIN);
    if bw < 4 || bh < 4 {
        return None;
    }
    let mut bins = vec![0.0f64; bw * bh];
    for by in 0..bh {
        for bx in 0..bw {
            let (x, y) = (bx * BIN, by * BIN);
            bins[by * bw + bx] = (lum(frame, x, y) + lum(frame, x + 2, y) + lum(frame, x, y + 2) + lum(frame, x + 2, y + 2)) / 4.0;
        }
    }
    let mut sorted = bins.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let sky = sorted[sorted.len() / 2];
    let mut dev: Vec<f64> = sorted.iter().map(|v| (v - sky).abs()).collect();
    dev.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let sigma = 1.4826 * dev[dev.len() / 2];
    let (top, &peak) = bins.iter().enumerate().max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))?;
    let level = peak - sky;
    if level <= 8.0 * sigma.max(1.0) {
        return None;
    }
    let thresh = sky + 0.25 * level;
    let mut seen = vec![false; bw * bh];
    let mut stack = vec![top];
    seen[top] = true;
    let (mut x0, mut x1, mut y0, mut y1, mut count) = (bw, 0usize, bh, 0usize, 0usize);
    while let Some(i) = stack.pop() {
        let (bx, by) = (i % bw, i / bw);
        count += 1;
        x0 = x0.min(bx);
        x1 = x1.max(bx);
        y0 = y0.min(by);
        y1 = y1.max(by);
        for ny in by.saturating_sub(1)..=(by + 1).min(bh - 1) {
            for nx in bx.saturating_sub(1)..=(bx + 1).min(bw - 1) {
                let j = ny * bw + nx;
                if !seen[j] && bins[j] > thresh {
                    seen[j] = true;
                    stack.push(j);
                }
            }
        }
    }
    if count < 3 || count * 10 > bw * bh * 6 {
        return None;
    }
    Some(PlanetBlob {
        cx: (x0 + x1 + 1) as f64 * BIN as f64 / 2.0,
        cy: (y0 + y1 + 1) as f64 * BIN as f64 / 2.0,
        half_w: (x1 - x0 + 1) as f64 * BIN as f64 / 2.0,
        half_h: (y1 - y0 + 1) as f64 * BIN as f64 / 2.0,
        sky,
        level,
    })
}

/// Sharpness of a planet (or any small bright target) for focusing: the root-mean-square gradient of the target and its edge, in a window
/// that follows it, divided by its brightness above the sky, so it does not change with exposure, gain or where the target sits in the frame.
/// A small planet has no surface texture worth scoring on its own, so the limb, the bands and the ring edges all count: they all blur together
/// when the focus is off. Pixel noise is taken off as in [`sharpness`], with a median so the limb does not inflate it. `None` when no planet is
/// found. Only comparisons within one session mean anything: the number depends on the planet's size and the sampling.
pub fn planet_sharpness(frame: &Frame) -> Option<f64> {
    let b = find_planet(frame)?;
    let scale = if frame.bayer.is_some() { 2 } else { 1 };
    let (w, h) = ((frame.width as usize / scale) as i64, (frame.height as usize / scale) as i64);
    let m = (0.25 * b.half_w.max(b.half_h)).max(8.0);
    let x0 = ((b.cx - b.half_w - m) as i64).clamp(1, w - 3);
    let x1 = ((b.cx + b.half_w + m) as i64).clamp(x0 + 2, w - 2);
    let y0 = ((b.cy - b.half_h - m) as i64).clamp(1, h - 3);
    let y1 = ((b.cy + b.half_h + m) as i64).clamp(y0 + 2, h - 2);
    let q = |x: i64, y: i64| (lum(frame, x as usize, y as usize) - b.sky).max(0.0);
    let lo = 0.1 * b.level;
    let (mut sum_g2, mut n, mut sum_bright, mut nb) = (0.0f64, 0u64, 0.0f64, 0u64);
    let mut resp: Vec<f64> = Vec::new();
    for y in y0..y1 {
        for x in x0..x1 {
            let (p, r, d) = (q(x, y), q(x + 1, y), q(x, y + 1));
            if p.max(r).max(d) < lo {
                continue;
            }
            sum_g2 += (r - p) * (r - p) + (d - p) * (d - p);
            n += 1;
            if p > 0.5 * b.level {
                sum_bright += p;
                nb += 1;
                if x % 3 == 1 && y % 3 == 1 {
                    let l = |dx: i64, dy: i64| lum(frame, (x + dx) as usize, (y + dy) as usize);
                    resp.push((l(-1, -1) + l(1, -1) + l(-1, 1) + l(1, 1) - 2.0 * (l(0, -1) + l(-1, 0) + l(1, 0) + l(0, 1)) + 4.0 * l(0, 0)).abs());
                }
            }
        }
    }
    if n < 150 || nb < 50 || sum_bright <= 0.0 {
        return None;
    }
    resp.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    // the mask has coefficients whose squares sum to 36, so white noise s gives a response of standard deviation 6 s; the median of |N(0, 6 s)| is 0.6745 * 6 s
    let sigma = resp.get(resp.len() / 2).map_or(0.0, |m| m / (0.6745 * 6.0));
    let energy = (sum_g2 / n as f64 - 4.0 * sigma * sigma).max(0.0);
    Some(energy.sqrt() / (sum_bright / nb as f64) * 100.0)
}

/// The "lucky" score of a burst and its error: the mean of the sharpest frames (`LUCKY_FRACTION`, at least `LUCKY_MIN`), and the
/// standard error of that mean. Differences between two bursts smaller than their errors are noise.
pub fn lucky_score(scores: &[f64]) -> Option<(f64, f64)> {
    if scores.is_empty() {
        return None;
    }
    let mut s = scores.to_vec();
    s.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let k = ((s.len() as f64 * LUCKY_FRACTION).ceil() as usize).max(LUCKY_MIN).min(s.len());
    let top = &s[..k];
    let mean = top.iter().sum::<f64>() / k as f64;
    let err = if k > 1 { (top.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (k as f64 - 1.0)).sqrt() / (k as f64).sqrt() } else { 0.0 };
    Some((mean, err))
}

/// The burst-to-burst noise of the score, from the history itself: how much two bursts at the same focus differ (seeing), which the error
/// of one burst (the scatter of its own best frames) does not include. Second differences cancel a steady trend, so a focuser turned at a
/// steady pace does not read as noise: for independent noise of size s their spread is s * sqrt(6). Uses the last 12 bursts; 0 until there
/// are 5. A median keeps one jump (a gust, a cloud) out of it.
pub fn between_burst_noise(scores: &[f64]) -> f64 {
    let tail = &scores[scores.len().saturating_sub(12)..];
    if tail.len() < 5 {
        return 0.0;
    }
    let mut d2: Vec<f64> = tail.windows(3).map(|w| (w[2] - 2.0 * w[1] + w[0]).abs()).collect();
    d2.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let med = d2[d2.len() / 2];
    1.4826 * med / 6f64.sqrt()
}

/// Smooths a history of `[t, score, err]` points with a 1-2-1 filter (ends use what they have) and returns the highest smoothed
/// point: (index, smoothed score, its error).
pub fn peak(history: &[[f64; 3]]) -> Option<(usize, f64, f64)> {
    let n = history.len();
    (0..n)
        .map(|i| {
            let (mut sum, mut wsum, mut e2) = (0.0, 0.0, 0.0);
            for (j, wt) in [(i.wrapping_sub(1), 1.0), (i, 2.0), (i + 1, 1.0)] {
                if j < n {
                    sum += wt * history[j][1];
                    wsum += wt;
                    e2 += (wt * history[j][2]).powi(2);
                }
            }
            (i, sum / wsum, e2.sqrt() / wsum)
        })
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: f64 = 250.0;

    /// A bright disc centred at `(cx, 400)` with radius `R`, an edge blurred over `edge_blur` pixels, and surface texture whose
    /// contrast falls with `tex_blur` (a focus error blurs the texture; the limb has its own blur).
    fn disc(cx: f64, edge_blur: f64, tex_blur: f64, level: f64) -> Frame {
        let (w, h) = (900usize, 800usize);
        let mut d = Vec::with_capacity(w * h * 2);
        let att = (-0.5 * (tex_blur * 0.8f64).powi(2)).exp();
        for y in 0..h {
            for x in 0..w {
                let (dx, dy) = (x as f64 - cx, y as f64 - 400.0);
                let r = (dx * dx + dy * dy).sqrt();
                let edge = 1.0 / (1.0 + ((r - R) / edge_blur.max(0.3)).exp());
                let tex = 1.0 + 0.12 * att * ((dx * 0.8).sin() * (dy * 0.6).cos());
                let v = (edge * tex * level * 40000.0) as u16 + 200;
                d.extend_from_slice(&v.to_le_bytes());
            }
        }
        Frame::new(w as u32, h as u32, PixelFormat::Mono16, d)
    }
    fn score(f: &Frame, sun_x: f64) -> f64 {
        let win = Window::new(Some((400.0, 400.0, R)), f.width, f.height);
        sharpness(f, win, Some((sun_x, 400.0, R))).unwrap()
    }

    #[test]
    fn crisper_surface_detail_scores_higher() {
        let (a, b, c) = (score(&disc(400.0, 1.0, 0.5, 1.0), 400.0), score(&disc(400.0, 1.0, 2.0, 1.0), 400.0), score(&disc(400.0, 1.0, 4.0, 1.0), 400.0));
        assert!(a > b && b > c, "{a} {b} {c}");
    }

    #[test]
    fn pixel_noise_does_not_raise_the_score_or_hide_the_difference_between_sharp_and_soft() {
        fn noisy(mut f: Frame, amp: u32) -> Frame {
            let mut st = 0x9E3779B97F4A7C15u64;
            for c in f.data.chunks_exact_mut(2) {
                st ^= st << 13; st ^= st >> 7; st ^= st << 17;
                let v = (u16::from_le_bytes([c[0], c[1]]) as i64 + (st % (2 * amp as u64 + 1)) as i64 - amp as i64).clamp(0, 65535) as u16;
                c.copy_from_slice(&v.to_le_bytes());
            }
            f
        }
        let (clean, n) = (score(&disc(400.0, 1.0, 1.0, 1.0), 400.0), score(&noisy(disc(400.0, 1.0, 1.0, 1.0), 600), 400.0));
        assert!((clean - n).abs() / clean < 0.15, "{clean} vs {n}");
        let (sharp, soft) = (score(&noisy(disc(400.0, 1.0, 0.5, 1.0), 600), 400.0), score(&noisy(disc(400.0, 1.0, 4.0, 1.0), 600), 400.0));
        assert!(sharp > 2.0 * soft, "{sharp} vs {soft}");
    }

    #[test]
    fn the_limb_does_not_count() {
        let (a, b) = (score(&disc(400.0, 0.5, 1.0, 1.0), 400.0), score(&disc(400.0, 9.0, 1.0, 1.0), 400.0));
        assert!((a - b).abs() / a < 0.03, "a soft limb must not change the score: {a} vs {b}");
    }

    #[test]
    fn moving_the_sun_by_50_px_changes_the_score_by_a_few_percent_at_most() {
        let still = score(&disc(400.0, 1.0, 1.0, 1.0), 400.0);
        // the Sun, texture and all, shifted; the window stays where it was
        let moved = score(&disc(450.0, 1.0, 1.0, 1.0), 450.0);
        assert!((still - moved).abs() / still < 0.04, "{still} vs {moved}");
        // and the ordering of sharp and soft survives the shift
        assert!(score(&disc(450.0, 1.0, 0.5, 1.0), 450.0) > score(&disc(450.0, 1.0, 3.0, 1.0), 450.0));
    }

    #[test]
    fn the_score_does_not_depend_on_exposure() {
        let (a, b) = (score(&disc(400.0, 1.0, 1.0, 1.0), 400.0), score(&disc(400.0, 1.0, 1.0, 0.5), 400.0));
        assert!((a - b).abs() / a < 0.03, "{a} vs {b}");
    }

    #[test]
    fn a_dark_frame_has_no_score_and_a_sun_that_left_the_window_has_none_either() {
        let f = Frame::new(800, 800, PixelFormat::Mono16, vec![0; 800 * 800 * 2]);
        assert_eq!(sharpness(&f, Window::new(None, 800, 800), None), None);
        let win = Window::new(Some((400.0, 400.0, R)), 900, 800);
        assert_eq!(sharpness(&disc(400.0, 1.0, 1.0, 1.0), win, Some((880.0, 400.0, 20.0))), None);
    }

    #[test]
    fn lucky_score_is_the_mean_of_the_best_frames_with_its_error() {
        let s: Vec<f64> = (1..=40).map(|v| v as f64).collect();
        let (m, e) = lucky_score(&s).unwrap(); // the best 5 of 40: 36 to 40
        assert_eq!(m, 38.0);
        assert!((e - (2.5f64.sqrt() / 5.0f64.sqrt())).abs() < 1e-9, "{e}");
        assert_eq!(lucky_score(&[3.0]).unwrap().0, 3.0);
        assert_eq!(lucky_score(&[]), None);
        assert_eq!(lucky_score(&[1.0, 2.0, 3.0, 4.0]).unwrap().0, 3.0, "at least three frames count");
    }

    #[test]
    fn the_noise_between_bursts_ignores_a_steady_trend_and_sees_the_scatter() {
        // a steady climb: no noise
        let climb: Vec<f64> = (0..12).map(|i| 1.0 + 0.05 * i as f64).collect();
        assert!(between_burst_noise(&climb) < 1e-9);
        // the scatter seen on the real Sun (about 3.6 %): recovered to within 40 %
        let mut s = 7u64;
        let noisy: Vec<f64> = (0..12).map(|_| { s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); 1.1 + ((s >> 33) as f64 / (1u64 << 31) as f64 - 0.5) * 0.04 * 3.46 }).collect();
        let n = between_burst_noise(&noisy);
        assert!(n > 0.02 && n < 0.07, "{n}");
        assert_eq!(between_burst_noise(&[1.0, 1.1, 1.0]), 0.0, "not enough bursts yet");
    }

    #[test]
    fn the_peak_is_found_on_the_smoothed_curve_and_not_on_a_single_lucky_point() {
        let h: Vec<[f64; 3]> = [1.0, 1.1, 1.5, 3.0, 1.6, 1.4, 1.3, 1.9, 2.0, 1.9, 1.2].iter().enumerate().map(|(i, v)| [i as f64 * 2.5, *v, 0.1]).collect();
        let (i, ..) = peak(&h).unwrap();
        assert_eq!(i, 3, "the sharp single point is still the best: {i}");
        let h: Vec<[f64; 3]> = [1.0, 1.1, 1.5, 2.6, 1.0, 1.4, 1.9, 2.0, 2.1, 1.9, 1.2].iter().enumerate().map(|(i, v)| [i as f64 * 2.5, *v, 0.1]).collect();
        assert_eq!(peak(&h).unwrap().0, 8, "a broad hill beats a spike");
    }

    /// A planet `r` pixels in radius at `(cx, cy)` in a 900 x 800 frame, a limb softened by `blur` pixels, two bands whose contrast falls with it.
    fn planet(cx: f64, cy: f64, r: f64, blur: f64, level: f64) -> Frame {
        let (w, h) = (900usize, 800usize);
        let att = (-0.5 * (blur * 0.35f64).powi(2)).exp();
        let mut d = Vec::with_capacity(w * h * 2);
        for y in 0..h {
            for x in 0..w {
                let (dx, dy) = (x as f64 - cx, y as f64 - cy);
                let edge = 1.0 / (1.0 + (((dx * dx + dy * dy).sqrt() - r) / (0.3 + 0.5 * blur)).exp());
                let bands = 1.0 + 0.2 * att * (dy * 0.45).sin();
                d.extend_from_slice(&((edge * bands * level * 30000.0) as u16 + 300).to_le_bytes());
            }
        }
        Frame::new(w as u32, h as u32, PixelFormat::Mono16, d)
    }

    #[test]
    fn a_crisper_planet_scores_higher_wherever_it_sits_and_whatever_the_exposure() {
        let (a, b, c) = (planet(450.0, 400.0, 40.0, 0.5, 1.0), planet(450.0, 400.0, 40.0, 2.0, 1.0), planet(450.0, 400.0, 40.0, 5.0, 1.0));
        let (sa, sb, sc) = (planet_sharpness(&a).unwrap(), planet_sharpness(&b).unwrap(), planet_sharpness(&c).unwrap());
        assert!(sa > sb && sb > sc, "{sa} {sb} {sc}");
        let moved = planet_sharpness(&planet(180.0, 600.0, 40.0, 0.5, 1.0)).unwrap();
        assert!((moved - sa).abs() / sa < 0.05, "the window follows the planet: {moved} vs {sa}");
        let dim = planet_sharpness(&planet(450.0, 400.0, 40.0, 0.5, 0.4)).unwrap();
        assert!((dim - sa).abs() / sa < 0.05, "exposure does not matter: {dim} vs {sa}");
    }

    #[test]
    fn pixel_noise_does_not_hide_the_difference_between_a_sharp_and_a_soft_planet() {
        let noisy = |mut f: Frame| {
            let mut st = 0x9E3779B97F4A7C15u64;
            for c in f.data.chunks_exact_mut(2) {
                st ^= st << 13; st ^= st >> 7; st ^= st << 17;
                let v = (u16::from_le_bytes([c[0], c[1]]) as i64 + (st % 1201) as i64 - 600).clamp(0, 65535) as u16;
                c.copy_from_slice(&v.to_le_bytes());
            }
            f
        };
        let (clean, n) = (planet_sharpness(&planet(450.0, 400.0, 40.0, 1.0, 1.0)).unwrap(), planet_sharpness(&noisy(planet(450.0, 400.0, 40.0, 1.0, 1.0))).unwrap());
        assert!((clean - n).abs() / clean < 0.2, "{clean} vs {n}");
        let (sharp, soft) = (planet_sharpness(&noisy(planet(450.0, 400.0, 40.0, 0.5, 1.0))).unwrap(), planet_sharpness(&noisy(planet(450.0, 400.0, 40.0, 5.0, 1.0))).unwrap());
        assert!(sharp > 1.5 * soft, "{sharp} vs {soft}");
    }

    #[test]
    fn a_dark_frame_a_covered_telescope_and_a_flood_of_light_have_no_planet() {
        let dark = Frame::new(800, 800, PixelFormat::Mono16, vec![0; 800 * 800 * 2]);
        assert_eq!(planet_sharpness(&dark), None);
        let flat = Frame::new(800, 800, PixelFormat::Mono16, [0x00u8, 0x40].repeat(800 * 800));
        assert_eq!(planet_sharpness(&flat), None);
        assert!(planet_sharpness(&planet(450.0, 400.0, 3.0, 0.5, 1.0)).is_none(), "a point of light is not a planet");
    }
}
