//! The limb-point extraction as it was before the speed-up (issue #100), kept to prove the fast code finds the same points.
#![allow(dead_code)]
use super::{kasa, Frame, PixelFormat, DISC_STEP};

#[inline]
fn px8(f: &Frame, x: u32, y: u32) -> u8 {
    match f.format {
        PixelFormat::Mono8 => f.data[y as usize * f.width as usize + x as usize],
        PixelFormat::Mono16 => (f.pixel(x, y) >> 8) as u8,
    }
}

/// A pixel in 8-bit units (fractional for 16-bit data), before any stretch.
#[inline]
fn px_f(f: &Frame, x: u32, y: u32) -> f64 {
    match f.format {
        PixelFormat::Mono8 => f.data[y as usize * f.width as usize + x as usize] as f64,
        PixelFormat::Mono16 => f.pixel(x, y) as f64 / 256.0,
    }
}

/// How a dim frame is brightened for detection: the sensor's black level is subtracted, then the rest is multiplied.
#[derive(Copy, Clone)]
struct View { gain: f64, black: f64 }

/// A pixel in 8-bit units after brightening a dim frame.
#[inline]
fn pxs(f: &Frame, x: u32, y: u32, v: View) -> u8 {
    if v.gain == 1.0 { return px8(f, x, y); }
    ((px_f(f, x, y) - v.black).max(0.0) * v.gain).min(255.0) as u8
}

/// How much to brighten the frame so the Sun is detected the same in full sun and under cloud or a tree's shadow
/// (the light can drop a hundredfold within seconds, and the top 8 bits of a dim 16-bit frame hold almost nothing).
/// Only dim frames are brightened: the 99.5th percentile is brought to 128 (8-bit units). `None`: too dark to hold
/// a Sun at all (below the sensor noise).
fn view_gain(frame: &Frame) -> Option<View> {
    let mut hist = vec![0u32; 4096]; // 1/16 of an 8-bit unit per bin
    let mut n = 0u64;
    for y in (0..frame.height).step_by(DISC_STEP as usize * 2) {
        for x in (0..frame.width).step_by(DISC_STEP as usize * 2) {
            hist[((px_f(frame, x, y) * 16.0) as usize).min(4095)] += 1;
            n += 1;
        }
    }
    let pct = |q: f64| {
        let target = (n as f64 * q) as u64;
        let mut acc = 0u64;
        for (i, c) in hist.iter().enumerate() {
            acc += *c as u64;
            if acc >= target { return (i as f64 + 1.0) / 16.0; }
        }
        256.0
    };
    let p995 = pct(0.995);
    let floor = if frame.format == PixelFormat::Mono8 { 8.0 } else { 1.5 };
    if p995 < floor { return None; }
    if p995 >= 128.0 { return Some(View { gain: 1.0, black: 0.0 }); }
    // the sky's level (the sensor's black offset) is the low end of the histogram: take it away before multiplying
    let p2 = pct(0.02);
    let black = if p2 < 0.25 * p995 { (p2 - 0.1).max(0.0) } else { 0.0 };
    Some(View { gain: 128.0 / (p995 - black).max(0.5), black })
}

/// Otsu threshold of the (subsampled) frame, or `None` if the frame is not "bright Sun on darker sky".
///
/// Half-of-peak thresholds fail on H-alpha: bright plage and prominences push the peak well above the
/// typical disc level, putting the cut inside the disc. Otsu splits sky from disc wherever the
/// histogram's valley is. A frame needs two clearly different populations (upper class at least twice
/// as bright as the lower), otherwise there is no limb in view (e.g. a field entirely on the disc).
fn otsu_threshold(frame: &Frame, view: View) -> Option<u8> {
    // Histogram on a log-intensity axis (256 bins over ln(1+v), v in 0..=255): brightness structure on the
    // disc is multiplicative (plage, filaments), so sky-vs-disc then outweighs dark-disc-vs-plage.
    const LN256: f64 = 5.545_177_444_479_562;
    let bin_of = |v: u8| ((1.0 + v as f64).ln() / LN256 * 255.0).round() as usize;
    let value_of = |bin: usize| ((bin as f64 / 255.0 * LN256).exp() - 1.0).round().clamp(0.0, 255.0) as u8;

    let mut hist = [0u32; 256];
    let mut vals = [0u32; 256]; // true value histogram, for the final brightness check
    let mut n = 0u64;
    for y in (0..frame.height).step_by(DISC_STEP as usize) {
        for x in (0..frame.width).step_by(DISC_STEP as usize) {
            let v = pxs(frame, x, y, view);
            hist[bin_of(v)] += 1;
            vals[v as usize] += 1;
            n += 1;
        }
    }
    if n == 0 {
        return None;
    }
    let total_sum: f64 = hist.iter().enumerate().map(|(i, c)| i as f64 * *c as f64).sum();
    // When sky and disc are cleanly separated, every cut inside the gap scores the same: take the middle
    // of that plateau, not its edge (which would sit right at the sky level).
    let (mut w0, mut sum0, mut best) = (0f64, 0f64, -1.0f64);
    let (mut first, mut last) = (0usize, 0usize);
    for t in 0..255 {
        w0 += hist[t] as f64;
        sum0 += t as f64 * hist[t] as f64;
        let w1 = n as f64 - w0;
        if w0 == 0.0 || w1 == 0.0 {
            continue;
        }
        let (m0, m1) = (sum0 / w0, (total_sum - sum0) / w1);
        let between = w0 * w1 * (m0 - m1) * (m0 - m1);
        if between > best * (1.0 + 1e-9) {
            best = between;
            first = t;
            last = t;
        } else if (between - best).abs() <= best * 1e-9 {
            last = t;
        }
    }
    if best < 0.0 {
        return None;
    }
    let thr = value_of((first + last) / 2 + 1).max(1);
    let (mut lo_n, mut lo_s, mut hi_n, mut hi_s) = (0f64, 0f64, 0f64, 0f64);
    for (v, c) in vals.iter().enumerate() {
        if v < thr as usize { lo_n += *c as f64; lo_s += v as f64 * *c as f64 } else { hi_n += *c as f64; hi_s += v as f64 * *c as f64 }
    }
    let (lo, hi) = (lo_s / lo_n.max(1.0), hi_s / hi_n.max(1.0));
    // two clearly different populations, and the bright one must actually be bright
    if lo_n == 0.0 || hi_n == 0.0 || hi < 20.0 || hi < 2.0 * lo.max(1.0) {
        return None;
    }
    Some(thr)
}

/// The cut between the black sky and anything brighter than it (shadows, thin cloud, limb darkening included).
///
/// Otsu splits at the biggest brightness jump, which on a disc crossed by a tree's shadow is the shadow edge: the
/// shadowed part of the disc then counts as "sky", shadow edges become false limb points and the fit swings around.
/// The sky itself is a dark mode, so cut just above it instead. `None` if no frame fraction looks like sky.
fn sky_edge_threshold(frame: &Frame, view: View) -> Option<(u8, f64)> {
    let mut vals = [0u32; 256];
    let mut n = 0u64;
    for y in (0..frame.height).step_by(DISC_STEP as usize) {
        for x in (0..frame.width).step_by(DISC_STEP as usize) {
            vals[pxs(frame, x, y, view) as usize] += 1;
            n += 1;
        }
    }
    if n == 0 {
        return None;
    }
    // the sky: the tallest bin among the darkest values, extended up while the histogram has not died away
    let (mut peak, mut pc) = (0usize, 0u32);
    for v in 0..=40 {
        if vals[v] > pc { pc = vals[v]; peak = v; }
    }
    let nn = n as f64;
    let mut sky_hi = peak;
    while sky_hi < 250 && (vals[sky_hi + 1..=(sky_hi + 3).min(255)].iter().any(|&c| c as f64 > 0.04 * pc as f64)) {
        sky_hi += 1;
    }
    let thr = sky_hi + 3;
    let sky: u64 = vals[..=sky_hi].iter().map(|&c| c as u64).sum();
    let above: u64 = vals[thr.min(255)..].iter().map(|&c| c as u64).sum();
    let mean_above = vals.iter().enumerate().skip(thr.min(255)).map(|(v, &c)| v as f64 * c as f64).sum::<f64>() / above.max(1) as f64;
    if (sky as f64) < 0.02 * nn || (above as f64) < 0.02 * nn || mean_above < 2.0 * thr as f64 || mean_above < 6.0 || thr > 120 {
        return None;
    }
    Some((thr as u8, above as f64 / nn))
}

fn sun_threshold(frame: &Frame, view: View) -> Option<u8> {
    let otsu = otsu_threshold(frame, view);
    let Some((sky_thr, _)) = sky_edge_threshold(frame, view) else { return otsu };
    match otsu {
        None => Some(sky_thr),
        Some(o) if sky_thr < o => Some(sky_thr),
        _ => otsu,
    }
}

/// Centroid and equivalent radius of the bright disc.
/// Edge points of the bright region, excluding points where the region is merely cut off by the frame border.
pub fn limb_points(frame: &Frame, expected: Option<f64>) -> Vec<(f64, f64)> {
    let st = DISC_STEP;
    let Some(view) = view_gain(frame) else { return vec![] };
    let Some(thr) = sun_threshold(frame, view) else { return vec![] };
    let margin = 2 * st;
    let mut pts = vec![];
    let mut y = margin;
    while y + margin < frame.height {
        let mut x = margin;
        while x + margin < frame.width {
            if pxs(frame, x, y, view) >= thr {
                let edge = [(x - st, y), (x + st, y), (x, y - st), (x, y + st)].iter().any(|&(nx, ny)| pxs(frame, nx, ny, view) < thr);
                if edge {
                    pts.push((x as f64, y as f64));
                }
            }
            x += st;
        }
        y += st;
    }
    trim_outliers(pts, expected)
}

/// Keeps the edge points that lie on one circle and drops the rest: the edges of a shadow (a branch, a pole) or of a
/// cloud inside the disc can be a third of all points, too many for a least-squares fit or a median cut to ignore.
/// RANSAC: try circles through random point triples, keep the one most points agree with, then polish with those.
fn trim_outliers(pts: Vec<(f64, f64)>, expected: Option<f64>) -> Vec<(f64, f64)> {
    let free = ransac_circle(&pts, None);
    let near = expected.and_then(|e| ransac_circle(&pts, Some(e)));
    // a circle near the optics' radius wins unless it explains clearly fewer points than the free one
    let pick = match (near, free) {
        (Some(n), Some(f)) if n.3 * 10 >= f.3 * 6 => Some(n),
        (Some(n), None) => Some(n),
        (_, f) => f,
    };
    let Some((cx0, cy0, r0, inl)) = pick else { return pts };
    if inl < 30 { return pts; }
    let tol = 1.5 * DISC_STEP as f64;
    let (mut cx, mut cy, mut r) = (cx0, cy0, r0);
    let mut keep: Vec<(f64, f64)> = pts.iter().copied().filter(|p| ((p.0 - cx).hypot(p.1 - cy) - r).abs() <= tol).collect();
    // polish on the inliers, then take everything within the tolerance of the polished circle
    if let Some((x, y, rr)) = kasa(&keep) {
        if rr.is_finite() && (20.0..=20000.0).contains(&rr) { cx = x; cy = y; r = rr; }
    }
    let polished: Vec<(f64, f64)> = pts.iter().copied().filter(|p| ((p.0 - cx).hypot(p.1 - cy) - r).abs() <= tol).collect();
    if polished.len() >= 30 { keep = polished; }
    keep
}

/// The circle through three random points that most points lie on: (cx, cy, r, inliers). With `near`, only circles within
/// 20 % of that radius count.
fn ransac_circle(pts: &[(f64, f64)], near: Option<f64>) -> Option<(f64, f64, f64, usize)> {
    let n = pts.len();
    if n < 60 {
        return None;
    }
    let tol = 1.5 * DISC_STEP as f64; // the points sit on a grid of this spacing
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15 ^ n as u64;
    let mut next = || { seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17; (seed % n as u64) as usize };
    let mut best = None::<(f64, f64, f64, usize)>;
    for _ in 0..300 {
        let (a, b, c) = (pts[next()], pts[next()], pts[next()]);
        let d = 2.0 * (a.0 * (b.1 - c.1) + b.0 * (c.1 - a.1) + c.0 * (a.1 - b.1));
        if d.abs() < 1e-6 { continue; }
        let ux = ((a.0 * a.0 + a.1 * a.1) * (b.1 - c.1) + (b.0 * b.0 + b.1 * b.1) * (c.1 - a.1) + (c.0 * c.0 + c.1 * c.1) * (a.1 - b.1)) / d;
        let uy = ((a.0 * a.0 + a.1 * a.1) * (c.0 - b.0) + (b.0 * b.0 + b.1 * b.1) * (a.0 - c.0) + (c.0 * c.0 + c.1 * c.1) * (b.0 - a.0)) / d;
        let r = (a.0 - ux).hypot(a.1 - uy);
        if !(20.0..=20000.0).contains(&r) || near.is_some_and(|e| (r / e - 1.0).abs() > 0.2) { continue; }
        let inl = pts.iter().filter(|p| ((p.0 - ux).hypot(p.1 - uy) - r).abs() <= tol).count();
        if best.map_or(true, |b| inl > b.3) { best = Some((ux, uy, r, inl)); }
    }
    best
}

