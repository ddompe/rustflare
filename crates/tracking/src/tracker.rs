use serde::Serialize;
use rustflare_core::{Frame, PixelFormat, Vec2};
use thiserror::Error;

const ANCHOR_SEARCH_RADIUS: i32 = 20;
const REF_BLOCK_SIZE: i32 = 64;
/// Subsampling step for whole-disc detection.
const DISC_STEP: u32 = 4;
/// A candidate jump larger than this many disc radii is treated as an image artefact.
const MAX_JUMP_RADII: f64 = 0.5;

#[derive(Debug, Error)]
pub enum TrackError {
    #[error("no solar disc found in frame")]
    NoDisc,
    #[error("no usable limb arc found (is part of the solar edge in frame?)")]
    NoLimb,
    #[error("tracked feature is too close to the frame edge")]
    OutOfBounds,
}

#[derive(Copy, Clone, Debug, Serialize)]
pub struct Position {
    pub pos: Vec2,
    /// Disc radius in pixels (disc mode only).
    pub radius: Option<f64>,
    /// True if this frame's measurement was rejected as an outlier and the previous position kept.
    pub held: bool,
}

enum State {
    Disc { pos: Vec2, radius: f64 },
    /// Partial disc: centre inferred from the visible limb arc, radius locked after the first fit.
    Limb { pos: Vec2, radius: f64 },
    Anchor { pos: (i32, i32), block: Vec<u8> },
}

pub struct Tracker {
    state: State,
}

#[inline]
fn px8(f: &Frame, x: u32, y: u32) -> u8 {
    match f.format {
        PixelFormat::Mono8 => f.data[y as usize * f.width as usize + x as usize],
        PixelFormat::Mono16 => (f.pixel(x, y) >> 8) as u8,
    }
}

/// Otsu threshold of the (subsampled) frame, or `None` if the frame is not "bright Sun on darker sky".
///
/// Half-of-peak thresholds fail on H-alpha: bright plage and prominences push the peak well above the
/// typical disc level, putting the cut inside the disc. Otsu splits sky from disc wherever the
/// histogram's valley is. A frame needs two clearly different populations (upper class at least twice
/// as bright as the lower), otherwise there is no limb in view (e.g. a field entirely on the disc).
fn sun_threshold(frame: &Frame) -> Option<u8> {
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
            let v = px8(frame, x, y);
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

/// Centroid and equivalent radius of the bright disc.
pub fn find_disc(frame: &Frame) -> Option<(Vec2, f64)> {
    let thr = sun_threshold(frame)?;
    let (mut sx, mut sy, mut n) = (0f64, 0f64, 0f64);
    for y in (0..frame.height).step_by(DISC_STEP as usize) {
        for x in (0..frame.width).step_by(DISC_STEP as usize) {
            if px8(frame, x, y) >= thr {
                sx += x as f64;
                sy += y as f64;
                n += 1.0;
            }
        }
    }
    if n < 50.0 {
        return None;
    }
    let area_px = n * (DISC_STEP * DISC_STEP) as f64;
    Some((Vec2::new(sx / n, sy / n), (area_px / std::f64::consts::PI).sqrt()))
}

/// Where the Sun is in a frame, independent of any running tracker (for centring aids).
#[derive(Copy, Clone, Debug, Serialize)]
pub struct SunLocation {
    pub pos: Vec2,
    pub radius: f64,
    /// Part of the disc is outside the frame, so `pos`/`radius` come from a limb fit.
    pub partial: bool,
    /// Partial discs only: true if `radius` is the one expected from the optics, false if it was measured
    /// from the limb because the optics' radius did not fit the data.
    pub radius_from_optics: Option<bool>,
    /// Partial discs only: RMS residual of the limb fit (pixels).
    pub fit_rms: Option<f64>,
}

/// Finds the Sun: whole-disc centroid when the disc is fully in frame, limb fit when it is cut off.
/// `expected_radius` (pixels, from the optics) makes the partial-disc fit much more reliable; if the
/// data disagree with it, the radius is fitted freely instead.
pub fn locate_sun(frame: &Frame, expected_radius: Option<f64>) -> Option<SunLocation> {
    let thr = sun_threshold(frame)?;
    let (w, h) = (frame.width, frame.height);
    let st = DISC_STEP as usize;
    let touches_edge = (0..w).step_by(st).any(|x| px8(frame, x, 0) >= thr || px8(frame, x, h - 1) >= thr)
        || (0..h).step_by(st).any(|y| px8(frame, 0, y) >= thr || px8(frame, w - 1, y) >= thr);
    if !touches_edge {
        let (c, r) = find_disc(frame)?;
        return Some(SunLocation { pos: c, radius: r, partial: false, radius_from_optics: None, fit_rms: None });
    }
    let fit = fit_limb_auto(frame, expected_radius)?;
    Some(SunLocation { pos: fit.pos, radius: fit.radius, partial: true, radius_from_optics: Some(fit.radius_from_optics), fit_rms: Some(fit.rms) })
}

impl Tracker {
    /// Tracks the centroid of the whole solar disc. Works while the entire disc is in frame.
    pub fn sun_disc(frame: &Frame) -> Result<Tracker, TrackError> {
        let (pos, radius) = find_disc(frame).ok_or(TrackError::NoDisc)?;
        Ok(Tracker { state: State::Disc { pos, radius } })
    }

    /// Tracks the disc centre from the visible limb arc, so it keeps working when part of the
    /// Sun is outside the frame. If `known_radius` (pixels) is given it is used as-is; otherwise
    /// the radius is fitted from the first frame (needs a reasonably long arc in view) and then locked.
    pub fn limb(frame: &Frame, known_radius: Option<f64>) -> Result<Tracker, TrackError> {
        let fit = fit_limb_auto(frame, known_radius).ok_or(TrackError::NoLimb)?;
        Ok(Tracker { state: State::Limb { pos: fit.pos, radius: fit.radius } })
    }

    /// Tracks a surface feature (e.g. a sunspot) by block matching around `pos`.
    /// Use this when the disc is larger than the frame.
    pub fn anchor(frame: &Frame, pos: Vec2) -> Result<Tracker, TrackError> {
        let (x, y) = (pos.x.round() as i32, pos.y.round() as i32);
        let block = read_block(frame, x, y, REF_BLOCK_SIZE, REF_BLOCK_SIZE).ok_or(TrackError::OutOfBounds)?;
        Ok(Tracker { state: State::Anchor { pos: (x, y), block } })
    }

    pub fn mode(&self) -> &'static str {
        match self.state { State::Disc { .. } => "sun_disc", State::Limb { .. } => "limb", State::Anchor { .. } => "anchor" }
    }

    pub fn position(&self) -> Position {
        match &self.state {
            State::Disc { pos, radius } => Position { pos: *pos, radius: Some(*radius), held: false },
            State::Limb { pos, radius } => Position { pos: *pos, radius: Some(*radius), held: false },
            State::Anchor { pos, .. } => Position { pos: Vec2::new(pos.0 as f64, pos.1 as f64), radius: None, held: false },
        }
    }

    pub fn update(&mut self, frame: &Frame) -> Result<Position, TrackError> {
        match &mut self.state {
            State::Disc { pos, radius } => {
                let (new_pos, new_r) = find_disc(frame).ok_or(TrackError::NoDisc)?;
                // Vidoxide ignores implausible jumps (damaged frames); same idea, relative to disc size.
                if (new_pos - *pos).len() > MAX_JUMP_RADII * *radius {
                    return Ok(Position { pos: *pos, radius: Some(*radius), held: true });
                }
                *pos = new_pos;
                *radius = new_r;
                Ok(Position { pos: new_pos, radius: Some(new_r), held: false })
            }
            State::Limb { pos, radius } => {
                let (new_pos, _) = fit_limb(frame, Some(*pos), Some(*radius)).ok_or(TrackError::NoLimb)?;
                if (new_pos - *pos).len() > MAX_JUMP_RADII * *radius {
                    return Ok(Position { pos: *pos, radius: Some(*radius), held: true });
                }
                *pos = new_pos;
                Ok(Position { pos: new_pos, radius: Some(*radius), held: false })
            }
            State::Anchor { pos, block } => {
                update_anchor(pos, block, frame)?;
                Ok(Position { pos: Vec2::new(pos.0 as f64, pos.1 as f64), radius: None, held: false })
            }
        }
    }
}

/// Edge points of the bright region, excluding points where the region is merely cut off by the frame border.
fn limb_points(frame: &Frame) -> Vec<(f64, f64)> {
    let st = DISC_STEP;
    let Some(thr) = sun_threshold(frame) else { return vec![] };
    let margin = 2 * st;
    let mut pts = vec![];
    let mut y = margin;
    while y + margin < frame.height {
        let mut x = margin;
        while x + margin < frame.width {
            if px8(frame, x, y) >= thr {
                let edge = [(x - st, y), (x + st, y), (x, y - st), (x, y + st)].iter().any(|&(nx, ny)| px8(frame, nx, ny) < thr);
                if edge {
                    pts.push((x as f64, y as f64));
                }
            }
            x += st;
        }
        y += st;
    }
    pts
}

/// Algebraic (Kåsa) circle fit: x²+y² = 2ax + 2by + c.
fn kasa(pts: &[(f64, f64)]) -> Option<(f64, f64, f64)> {
    let n = pts.len() as f64;
    let (mx, my) = (pts.iter().map(|p| p.0).sum::<f64>() / n, pts.iter().map(|p| p.1).sum::<f64>() / n);
    let (mut suu, mut suv, mut svv, mut suuu, mut svvv, mut suvv, mut svuu) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    for p in pts {
        let (u, v) = (p.0 - mx, p.1 - my);
        suu += u * u; suv += u * v; svv += v * v;
        suuu += u * u * u; svvv += v * v * v; suvv += u * v * v; svuu += v * u * u;
    }
    let det = suu * svv - suv * suv;
    if det.abs() < 1e-9 {
        return None; // collinear
    }
    let (r1, r2) = (0.5 * (suuu + suvv), 0.5 * (svvv + svuu));
    let uc = (r1 * svv - r2 * suv) / det;
    let vc = (r2 * suu - r1 * suv) / det;
    let r = (uc * uc + vc * vc + (suu + svv) / n).sqrt();
    Some((mx + uc, my + vc, r))
}

/// Fits the solar limb. With `radius` fixed, only the centre is estimated (Gauss–Newton from
/// `start`, or from a Kåsa fit when no start is given); otherwise centre and radius come from Kåsa.
fn fit_limb(frame: &Frame, start: Option<Vec2>, radius: Option<f64>) -> Option<(Vec2, f64)> {
    fit_limb_pts(&limb_points(frame), start, radius)
}

/// RMS distance of the limb points from the circle (centre, radius): how well a radius explains the data.
pub fn limb_rms(pts: &[(f64, f64)], c: Vec2, r: f64) -> f64 {
    (pts.iter().map(|p| ((p.0 - c.x).hypot(p.1 - c.y) - r).powi(2)).sum::<f64>() / pts.len().max(1) as f64).sqrt()
}

/// A fitted solar limb and how good the fit is.
#[derive(Copy, Clone, Debug, Serialize)]
pub struct LimbFit {
    pub pos: Vec2,
    pub radius: f64,
    /// RMS residual of the limb points, pixels.
    pub rms: f64,
    /// The radius is the one expected from the optics (it fits the data about as well as a free fit).
    pub radius_from_optics: bool,
}

/// Fits the limb. The radius expected from the optics is used only if it explains the data about as well as
/// a free fit; otherwise the radius is measured from the data (the optics are probably off: a Barlow's real
/// factor depends on its spacing, so a nominal 3x can be 3.35x). Measured on real H-alpha frames.
pub fn fit_limb_auto(frame: &Frame, expected: Option<f64>) -> Option<LimbFit> {
    let pts = limb_points(frame);
    let free = fit_limb_pts(&pts, None, None).map(|(c, r)| (c, r, limb_rms(&pts, c, r)));
    if let Some(er) = expected {
        if let Some((c, r)) = fit_limb_pts(&pts, None, Some(er)) {
            let rms = limb_rms(&pts, c, r);
            if free.map_or(true, |(_, _, frms)| rms <= frms * 1.5 + 1.5) {
                return Some(LimbFit { pos: c, radius: r, rms, radius_from_optics: true });
            }
        }
    }
    free.map(|(c, r, rms)| LimbFit { pos: c, radius: r, rms, radius_from_optics: false })
}

/// Both fits side by side, for diagnostics: (free, with the expected radius), each (centre, radius, rms).
pub fn limb_fits(frame: &Frame, expected: Option<f64>) -> (Option<(Vec2, f64, f64)>, Option<(Vec2, f64, f64)>) {
    let pts = limb_points(frame);
    let free = fit_limb_pts(&pts, None, None).map(|(c, r)| (c, r, limb_rms(&pts, c, r)));
    let fixed = expected.and_then(|er| fit_limb_pts(&pts, None, Some(er))).map(|(c, r)| (c, r, limb_rms(&pts, c, r)));
    (free, fixed)
}

fn fit_limb_pts(pts: &[(f64, f64)], start: Option<Vec2>, radius: Option<f64>) -> Option<(Vec2, f64)> {
    if pts.len() < 20 {
        return None;
    }
    let pts = pts.to_vec();
    let (kx, ky, kr) = kasa(&pts)?;
    let r = radius.unwrap_or(kr);
    if !(20.0..=20000.0).contains(&r) {
        return None;
    }
    let mut c = start.unwrap_or(Vec2::new(kx, ky));
    if radius.is_some() {
        for _ in 0..15 {
            // minimise sum (|p - c| - r)^2 : step = mean of residual * unit vector
            let (mut gx, mut gy, mut n) = (0.0, 0.0, 0.0);
            for p in &pts {
                let (dx, dy) = (p.0 - c.x, p.1 - c.y);
                let d = dx.hypot(dy).max(1e-9);
                let res = d - r;
                gx += res * dx / d;
                gy += res * dy / d;
                n += 1.0;
            }
            // d|p-c|/dc = -(p-c)/d, so moving c by +mean(res * unit) reduces the residual
            let (sx, sy) = (gx / n, gy / n);
            c = Vec2::new(c.x + sx, c.y + sy);
            if sx.hypot(sy) < 0.01 { break; }
        }
        // reject fits that do not agree with the data
        let rms = (pts.iter().map(|p| ((p.0 - c.x).hypot(p.1 - c.y) - r).powi(2)).sum::<f64>() / pts.len() as f64).sqrt();
        if rms > 0.05 * r { return None; }
    } else {
        // The algebraic (Kåsa) fit is biased on a short arc (real H-alpha data: radius +11 %, centre ~140 px
        // off). Refine centre AND radius on the geometric distance with Gauss-Newton, starting from it.
        let (mut cx, mut cy, mut rr) = (kx, ky, kr);
        for _ in 0..30 {
            let (mut a, mut b) = ([[0f64; 3]; 3], [0f64; 3]);
            for p in &pts {
                let (dx, dy) = (p.0 - cx, p.1 - cy);
                let d = dx.hypot(dy).max(1e-9);
                let j = [-dx / d, -dy / d, -1.0];
                let res = d - rr;
                for i in 0..3 {
                    b[i] -= j[i] * res;
                    for k in 0..3 {
                        a[i][k] += j[i] * j[k];
                    }
                }
            }
            let Some(delta) = solve3(a, b) else { break };
            cx += delta[0];
            cy += delta[1];
            rr += delta[2];
            if delta[0].hypot(delta[1]).max(delta[2].abs()) < 0.01 {
                break;
            }
        }
        if rr.is_finite() && (20.0..=20000.0).contains(&rr) && cx.is_finite() && cy.is_finite() {
            return Some((Vec2::new(cx, cy), rr));
        }
        c = Vec2::new(kx, ky);
    }
    Some((c, r))
}

/// Solves a 3x3 linear system (Gaussian elimination with partial pivoting).
fn solve3(mut a: [[f64; 3]; 3], mut b: [f64; 3]) -> Option<[f64; 3]> {
    for col in 0..3 {
        let piv = (col..3).max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))?;
        if a[piv][col].abs() < 1e-12 {
            return None;
        }
        a.swap(col, piv);
        b.swap(col, piv);
        for row in col + 1..3 {
            let f = a[row][col] / a[col][col];
            for k in col..3 {
                a[row][k] -= f * a[col][k];
            }
            b[row] -= f * b[col];
        }
    }
    let mut x = [0f64; 3];
    for row in (0..3).rev() {
        let s: f64 = (row + 1..3).map(|k| a[row][k] * x[k]).sum();
        x[row] = (b[row] - s) / a[row][row];
    }
    Some(x)
}

/// Reads a `w`x`h` block centred on (cx, cy) as 8-bit.
fn read_block(f: &Frame, cx: i32, cy: i32, w: i32, h: i32) -> Option<Vec<u8>> {
    let (x0, y0) = (cx - w / 2, cy - h / 2);
    if x0 < 0 || y0 < 0 || x0 + w > f.width as i32 || y0 + h > f.height as i32 {
        return None;
    }
    let mut v = Vec::with_capacity((w * h) as usize);
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            v.push(px8(f, x as u32, y as u32));
        }
    }
    Some(v)
}

fn sum_sq_diff(f: &Frame, cx: i32, cy: i32, block: &[u8]) -> Option<u64> {
    let (x0, y0) = (cx - REF_BLOCK_SIZE / 2, cy - REF_BLOCK_SIZE / 2);
    if x0 < 0 || y0 < 0 || x0 + REF_BLOCK_SIZE > f.width as i32 || y0 + REF_BLOCK_SIZE > f.height as i32 {
        return None;
    }
    let mut sum = 0u64;
    for by in 0..REF_BLOCK_SIZE {
        for bx in 0..REF_BLOCK_SIZE {
            let a = px8(f, (x0 + bx) as u32, (y0 + by) as u32) as i32;
            let b = block[(by * REF_BLOCK_SIZE + bx) as usize] as i32;
            sum += ((a - b) * (a - b)) as u64;
        }
    }
    Some(sum)
}

/// Coarse-to-fine block matching (ported from Vidoxide's `update_anchor`).
fn update_anchor(anchor: &mut (i32, i32), block: &[u8], frame: &Frame) -> Result<(), TrackError> {
    let r = ANCHOR_SEARCH_RADIUS + 2;
    let half = REF_BLOCK_SIZE / 2;
    if anchor.0 - r - half < 0 || anchor.1 - r - half < 0
        || anchor.0 + r + half > frame.width as i32 || anchor.1 + r + half > frame.height as i32 {
        return Err(TrackError::OutOfBounds);
    }
    let mut best = *anchor;
    let (mut xmin, mut xmax) = (anchor.0 - ANCHOR_SEARCH_RADIUS, anchor.0 + ANCHOR_SEARCH_RADIUS);
    let (mut ymin, mut ymax) = (anchor.1 - ANCHOR_SEARCH_RADIUS, anchor.1 + ANCHOR_SEARCH_RADIUS);
    let mut step = 2;
    while step > 0 {
        let mut min_sum = u64::MAX;
        let mut y = ymin;
        while y <= ymax {
            let mut x = xmin;
            while x <= xmax {
                if let Some(s) = sum_sq_diff(frame, x, y, block) {
                    if s < min_sum { min_sum = s; best = (x, y); }
                }
                x += step;
            }
            y += step;
        }
        xmin = best.0 - step; xmax = best.0 + step;
        ymin = best.1 - step; ymax = best.1 + step;
        step /= 2;
    }
    *anchor = best;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disc_frame(cx: f64, cy: f64, r: f64) -> Frame {
        let (w, h) = (640u32, 480u32);
        let mut d = vec![5u8; (w * h) as usize];
        for y in 0..h {
            for x in 0..w {
                let (dx, dy) = (x as f64 - cx, y as f64 - cy);
                if dx * dx + dy * dy < r * r {
                    // texture so the anchor tracker has something to lock on
                    d[(y * w + x) as usize] = 150 + ((x * 7 + y * 13) % 50) as u8;
                }
            }
        }
        Frame::new(w, h, PixelFormat::Mono8, d)
    }

    #[test]
    fn disc_centroid_and_radius() {
        let (c, r) = find_disc(&disc_frame(300.0, 200.0, 100.0)).unwrap();
        assert!((c.x - 300.0).abs() < 2.0 && (c.y - 200.0).abs() < 2.0, "{c:?}");
        assert!((r - 100.0).abs() < 4.0, "{r}");
    }

    #[test]
    fn disc_tracker_follows_and_rejects_jumps() {
        let mut t = Tracker::sun_disc(&disc_frame(300.0, 200.0, 100.0)).unwrap();
        let p = t.update(&disc_frame(310.0, 205.0, 100.0)).unwrap();
        assert!(!p.held && (p.pos.x - 310.0).abs() < 2.0);
        let p = t.update(&disc_frame(500.0, 205.0, 100.0)).unwrap();
        assert!(p.held, "a 190px jump must be held");
    }

    #[test]
    fn limb_tracks_partial_disc() {
        // Disc of radius 400 centred at (550, 250) in a 640x480 frame: the right/top parts are cut off.
        let (r, mut t) = (400.0, None);
        let f0 = disc_frame(550.0, 250.0, r);
        let tr = Tracker::limb(&f0, None).unwrap();
        let p0 = tr.position();
        assert!((p0.pos.x - 550.0).abs() < 8.0 && (p0.pos.y - 250.0).abs() < 8.0, "{:?}", p0);
        assert!((p0.radius.unwrap() - r).abs() < 15.0, "{:?}", p0.radius);
        t.replace(tr);
        let mut tr = t.unwrap();
        // shift by (12, -9): the full-disc centroid would be badly biased here, the limb fit must not be
        let p = tr.update(&disc_frame(562.0, 241.0, r)).unwrap();
        assert!((p.pos.x - 562.0).abs() < 4.0 && (p.pos.y - 241.0).abs() < 4.0, "{:?}", p);
        // and the plain centroid really is biased on this frame
        let (c, _) = find_disc(&disc_frame(562.0, 241.0, r)).unwrap();
        assert!((c.x - 562.0).abs() > 20.0, "centroid not biased?? {c:?}");
    }

    #[test]
    fn locate_sun_whole_and_partial() {
        let whole = locate_sun(&disc_frame(320.0, 240.0, 100.0), None).unwrap();
        assert!(!whole.partial && (whole.pos.x - 320.0).abs() < 3.0 && (whole.radius - 100.0).abs() < 5.0);
        let part = locate_sun(&disc_frame(550.0, 250.0, 400.0), None).unwrap();
        assert!(part.partial && (part.pos.x - 550.0).abs() < 10.0 && (part.pos.y - 250.0).abs() < 10.0, "{part:?}");
        assert!(locate_sun(&Frame::new(64, 64, PixelFormat::Mono8, vec![3; 4096]), None).is_none());
    }

    /// H-alpha-like disc: mid-brightness with bright plage (peak 250) and dark filaments (~60) on a dark sky (8).
    fn halpha(cx: f64, cy: f64, r: f64) -> Frame {
        let (w, h) = (640u32, 480u32);
        let mut d = vec![8u8; (w * h) as usize];
        for y in 0..h {
            for x in 0..w {
                let (dx, dy) = (x as f64 - cx, y as f64 - cy);
                if dx * dx + dy * dy < r * r {
                    let t = ((dx / 23.0).sin() * (dy / 31.0).cos() + 1.0) / 2.0; // 0..1 structure
                    let v = 60.0 + 190.0 * t.powi(3); // dark filaments ~60, bright plage up to 250
                    d[(y * w + x) as usize] = v as u8;
                }
            }
        }
        Frame::new(w, h, PixelFormat::Mono8, d)
    }

    #[test]
    fn halpha_disc_survives_bright_plage() {
        // A "half of peak" threshold would be ~125 and slice through the disc texture.
        let s = locate_sun(&halpha(320.0, 240.0, 150.0), None).unwrap();
        assert!(!s.partial);
        assert!((s.pos.x - 320.0).abs() < 4.0 && (s.pos.y - 240.0).abs() < 4.0, "{s:?}");
        assert!((s.radius - 150.0).abs() < 8.0, "{s:?}");
        // partial H-alpha disc, with and without the radius from the optics
        let free = locate_sun(&halpha(600.0, 300.0, 380.0), None).unwrap();
        let known = locate_sun(&halpha(600.0, 300.0, 380.0), Some(380.0)).unwrap();
        assert!(free.partial && known.partial);
        assert!((known.pos.x - 600.0).abs() < 6.0 && (known.pos.y - 300.0).abs() < 6.0, "known {known:?}");
        assert!((free.pos.x - 600.0).abs() < 25.0 && (free.pos.y - 300.0).abs() < 25.0, "free {free:?}");
    }

    #[test]
    fn field_entirely_on_the_disc_has_no_limb() {
        // textured but uniformly bright: no sky in view, so neither disc nor limb can be found
        let all_disc = Frame::new(64, 64, PixelFormat::Mono8, (0..4096).map(|i| 150 + (i % 40) as u8).collect());
        assert!(locate_sun(&all_disc, None).is_none());
        assert!(Tracker::limb(&all_disc, None).is_err());
    }

    #[test]
    fn wrong_optics_radius_is_overridden_by_the_data_and_a_right_one_is_kept() {
        // Partial disc of radius 400 whose limb arcs across the whole frame width (sag 160 px); the "optics" claim
        // 360 (10 % too small, like a 3x Barlow that is really 3.35x). On a much shorter arc a wrong radius fits
        // nearly as well (the centre compensates), so the data cannot tell and the hint is rightly kept.
        let f = disc_frame(320.0, 640.0, 400.0);
        let wrong = fit_limb_auto(&f, Some(360.0)).unwrap();
        assert!(!wrong.radius_from_optics, "trusted an optics radius that does not fit the limb");
        assert!((wrong.radius - 400.0).abs() < 12.0, "measured radius {}", wrong.radius);
        let right = fit_limb_auto(&f, Some(400.0)).unwrap();
        assert!(right.radius_from_optics && (right.radius - 400.0).abs() < 1e-6);
        // the tracker starts from the measured radius, not the wrong hint
        let t = Tracker::limb(&f, Some(360.0)).unwrap();
        assert!((t.position().radius.unwrap() - 400.0).abs() < 12.0);
    }

    #[test]
    fn limb_needs_an_arc() {
        let f = Frame::new(64, 64, PixelFormat::Mono8, vec![3; 64 * 64]);
        assert!(Tracker::limb(&f, None).is_err());
    }

    #[test]
    fn dark_frame_has_no_disc() {
        let f = Frame::new(64, 64, PixelFormat::Mono8, vec![3; 64 * 64]);
        assert!(find_disc(&f).is_none());
    }

    /// Disc whose surface texture is attached to the disc, like a sunspot field.
    fn textured(cx: i32, cy: i32) -> Frame {
        let (w, h) = (640u32, 480u32);
        let mut d = vec![5u8; (w * h) as usize];
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                let (dx, dy) = (x - cx, y - cy);
                if dx * dx + dy * dy < 150 * 150 {
                    // smooth, spatially-correlated structure (like real solar features)
                    let (fx, fy) = (dx as f64, dy as f64);
                    let v = 150.0 + 35.0 * (fx / 9.0 + fy / 13.0).sin() + 35.0 * (fx / 5.5 - fy / 7.0 + 1.0).sin()
                        + 25.0 * (fx / 17.0 + fy / 3.7).sin();
                    d[(y * w as i32 + x) as usize] = v as u8;
                }
            }
        }
        Frame::new(w, h, PixelFormat::Mono8, d)
    }

    #[test]
    fn anchor_follows_texture() {
        let mut t = Tracker::anchor(&textured(300, 240), Vec2::new(300.0, 240.0)).unwrap();
        let p = t.update(&textured(307, 235)).unwrap();
        assert_eq!((p.pos.x, p.pos.y), (307.0, 235.0));
    }
}
