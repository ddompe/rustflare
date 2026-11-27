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
    #[error("the picture went dark (a shadow or a cloud over the telescope): nothing can be measured")]
    Dark,
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
    /// A small bright disc (a planet): the centroid of the brightest blob, radius from its area.
    Planet { pos: Vec2, radius: f64 },
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

/// The frame sampled every `DISC_STEP` pixels in both directions, already brightened: what the thresholds, the disc centroid and
/// the edge scan all read, so the frame's pixels are touched once (not once per use, each with a format match and a conversion).
struct Samples { gw: usize, gh: usize, px: Vec<u8> }

fn sample_grid(frame: &Frame, view: View) -> Samples {
    let st = DISC_STEP as usize;
    let (w, h) = (frame.width as usize, frame.height as usize);
    let (gw, gh) = (w.div_ceil(st), h.div_ceil(st));
    let mut px = Vec::with_capacity(gw * gh);
    let plain = view.gain == 1.0;
    let bright = |v: f64| ((v - view.black).max(0.0) * view.gain).min(255.0) as u8;
    for gy in 0..gh {
        let y = gy * st;
        match frame.format {
            PixelFormat::Mono8 => {
                let row = &frame.data[y * w..(y + 1) * w];
                if plain { px.extend(row.iter().step_by(st).copied()) } else { px.extend(row.iter().step_by(st).map(|&v| bright(v as f64))) }
            }
            PixelFormat::Mono16 => {
                let row = &frame.data[2 * y * w..2 * (y + 1) * w];
                if plain { px.extend(row.chunks_exact(2).step_by(st).map(|c| c[1])) }
                else { px.extend(row.chunks_exact(2).step_by(st).map(|c| bright(u16::from_le_bytes([c[0], c[1]]) as f64 / 256.0))) }
            }
        }
    }
    Samples { gw, gh, px }
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
fn otsu_threshold(grid: &Samples) -> Option<u8> {
    // Histogram on a log-intensity axis (256 bins over ln(1+v), v in 0..=255): brightness structure on the
    // disc is multiplicative (plage, filaments), so sky-vs-disc then outweighs dark-disc-vs-plage.
    const LN256: f64 = 5.545_177_444_479_562;
    let mut bins = [0usize; 256];
    for (v, b) in bins.iter_mut().enumerate() { *b = ((1.0 + v as f64).ln() / LN256 * 255.0).round() as usize; }
    let value_of = |bin: usize| ((bin as f64 / 255.0 * LN256).exp() - 1.0).round().clamp(0.0, 255.0) as u8;

    let mut hist = [0u32; 256];
    let mut vals = [0u32; 256]; // true value histogram, for the final brightness check
    let n = grid.px.len() as u64;
    for &v in &grid.px { vals[v as usize] += 1; }
    for (v, &c) in vals.iter().enumerate() { hist[bins[v]] += c; }
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
fn sky_edge_threshold(grid: &Samples) -> Option<(u8, f64)> {
    let mut vals = [0u32; 256];
    let n = grid.px.len() as u64;
    for &v in &grid.px { vals[v as usize] += 1; }
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

fn sun_threshold(grid: &Samples) -> Option<u8> {
    let otsu = otsu_threshold(grid);
    let Some((sky_thr, _)) = sky_edge_threshold(grid) else { return otsu };
    match otsu {
        None => Some(sky_thr),
        Some(o) if sky_thr < o => Some(sky_thr),
        _ => otsu,
    }
}

/// Centroid and equivalent radius of the bright disc.
pub fn find_disc(frame: &Frame) -> Option<(Vec2, f64)> {
    let view = view_gain(frame)?;
    let grid = sample_grid(frame, view);
    let thr = sun_threshold(&grid)?;
    let (mut sx, mut sy, mut n) = (0f64, 0f64, 0f64);
    for gy in 0..grid.gh {
        for (gx, &v) in grid.px[gy * grid.gw..(gy + 1) * grid.gw].iter().enumerate() {
            if v >= thr {
                sx += (gx * DISC_STEP as usize) as f64;
                sy += (gy * DISC_STEP as usize) as f64;
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
    let view = view_gain(frame)?;
    let thr = sun_threshold(&sample_grid(frame, view))?;
    let (w, h) = (frame.width, frame.height);
    let st = DISC_STEP as usize;
    let touches_edge = (0..w).step_by(st).any(|x| pxs(frame, x, 0, view) >= thr || pxs(frame, x, h - 1, view) >= thr)
        || (0..h).step_by(st).any(|y| pxs(frame, 0, y, view) >= thr || pxs(frame, w - 1, y, view) >= thr);
    if !touches_edge {
        let (c, r) = find_disc(frame)?;
        return Some(SunLocation { pos: c, radius: r, partial: false, radius_from_optics: None, fit_rms: None });
    }
    let fit = fit_limb_auto(frame, expected_radius)?;
    Some(SunLocation { pos: fit.pos, radius: fit.radius, partial: true, radius_from_optics: Some(fit.radius_from_optics), fit_rms: Some(fit.rms) })
}

/// Where a small bright disc (a planet) is, independent of any tracker: the centroid of the brightest blob.
/// The sky level and its noise come from a sparse grid; the brightest 4x4 block seeds a flood fill of everything above half way
/// a quarter of the way from the sky to that block, and the centroid is weighted by brightness above the sky. `near` (last position and radius)
/// restricts the search to its neighbourhood. `None` when nothing stands out clearly (under 8 sigma and half a percent of full scale
/// above the sky), when the blob is under 7 px in area (a star, a hot pixel) or larger than a tenth of the frame (that is the Sun or
/// the Moon: use the disc detector). The radius is that of a disc of the blob's area at that level.
pub fn locate_planet(frame: &Frame, near: Option<(Vec2, f64)>) -> Option<SunLocation> {
    let (w, h) = (frame.width as usize, frame.height as usize);
    if w < 32 || h < 32 { return None; }
    let full = frame.format.full_scale();
    let fv = |x: usize, y: usize| frame.pixel(x as u32, y as u32) as f64;
    let mut grid: Vec<f64> = Vec::with_capacity((w / 8 + 1) * (h / 8 + 1));
    for y in (0..h).step_by(8) { for x in (0..w).step_by(8) { grid.push(fv(x, y)); } }
    grid.sort_by(|a, b| a.total_cmp(b));
    let bg = grid[grid.len() / 2];
    let mut dev: Vec<f64> = grid.iter().map(|v| (v - bg).abs()).collect();
    dev.sort_by(|a, b| a.total_cmp(b));
    let sigma = (1.4826 * dev[dev.len() / 2]).max(full / 8192.0);
    // the search window
    let (x0, y0, x1, y1) = match near {
        Some((p, r)) => {
            let half = (6.0 * r).max(60.0);
            (((p.x - half).max(0.0) as usize) & !3, ((p.y - half).max(0.0) as usize) & !3, ((p.x + half).min(w as f64 - 1.0)) as usize, ((p.y + half).min(h as f64 - 1.0)) as usize)
        }
        None => (0, 0, w - 1, h - 1),
    };
    // the brightest 4x4 block
    let (mut best, mut bx, mut by) = (f64::MIN, 0usize, 0usize);
    let mut y = y0;
    while y + 4 <= y1 + 1 {
        let mut x = x0;
        while x + 4 <= x1 + 1 {
            let mut sum = 0.0;
            for yy in y..y + 4 { for xx in x..x + 4 { sum += fv(xx, yy); } }
            if sum > best { best = sum; bx = x; by = y; }
            x += 4;
        }
        y += 4;
    }
    if best == f64::MIN { return None; }
    let bm = best / 16.0;
    if bm - bg < (8.0 * sigma).max(0.005 * full) { return None; }
    // the brightest pixel of that block seeds the fill
    let (mut sx, mut sy, mut sv) = (bx, by, f64::MIN);
    for yy in by..by + 4 { for xx in bx..bx + 4 { let v = fv(xx, yy); if v > sv { sv = v; sx = xx; sy = yy; } } }
    // a quarter of the way up (not half): dark bands and a soft edge then move the centroid less; never below 4 sigma of noise
    let thr = bg + (0.25 * (bm - bg)).max(4.0 * sigma);
    let limit = ((w * h) / 10).min(40_000);
    let side = 241usize; // the blob must fit in a window of this size around the seed
    let (ox, oy) = (sx as i64 - 120, sy as i64 - 120);
    let mut seen = vec![false; side * side];
    let mut stack = vec![(sx, sy)];
    seen[120 * side + 120] = true;
    let (mut n, mut sw, mut swx, mut swy) = (0usize, 0.0, 0.0, 0.0);
    while let Some((x, y)) = stack.pop() {
        let v = fv(x, y);
        n += 1;
        let wgt = (v - bg).max(0.0);
        sw += wgt; swx += wgt * x as f64; swy += wgt * y as f64;
        if n > limit { return None; }
        for (dx, dy) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1)] {
            let (nx, ny) = (x as i64 + dx, y as i64 + dy);
            if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 { continue; }
            let (lx, ly) = (nx - ox, ny - oy);
            if lx < 0 || ly < 0 || lx >= side as i64 || ly >= side as i64 { return None; } // too big for a planet
            let k = ly as usize * side + lx as usize;
            if seen[k] { continue; }
            if fv(nx as usize, ny as usize) >= thr { seen[k] = true; stack.push((nx as usize, ny as usize)); }
        }
    }
    if n < 7 || sw <= 0.0 { return None; }
    // the pixel centres are at integer coordinates, as everywhere else in this module
    Some(SunLocation { pos: Vec2::new(swx / sw, swy / sw), radius: (n as f64 / std::f64::consts::PI).sqrt(), partial: false, radius_from_optics: None, fit_rms: None })
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

    /// Tracks a planet: the centroid of the brightest small blob on a dark sky (see `locate_planet`). For a disc a few tens of
    /// pixels across, which has no limb worth fitting.
    pub fn planet(frame: &Frame) -> Result<Tracker, TrackError> {
        let l = locate_planet(frame, None).ok_or(TrackError::NoDisc)?;
        Ok(Tracker { state: State::Planet { pos: l.pos, radius: l.radius } })
    }

    /// Tracks a surface feature (e.g. a sunspot) by block matching around `pos`.
    /// Use this when the disc is larger than the frame.
    pub fn anchor(frame: &Frame, pos: Vec2) -> Result<Tracker, TrackError> {
        let (x, y) = (pos.x.round() as i32, pos.y.round() as i32);
        let block = read_block(frame, x, y, REF_BLOCK_SIZE, REF_BLOCK_SIZE).ok_or(TrackError::OutOfBounds)?;
        Ok(Tracker { state: State::Anchor { pos: (x, y), block } })
    }

    pub fn mode(&self) -> &'static str {
        match self.state { State::Disc { .. } => "sun_disc", State::Limb { .. } => "limb", State::Anchor { .. } => "anchor", State::Planet { .. } => "planet" }
    }

    pub fn position(&self) -> Position {
        match &self.state {
            State::Disc { pos, radius } => Position { pos: *pos, radius: Some(*radius), held: false },
            State::Limb { pos, radius } => Position { pos: *pos, radius: Some(*radius), held: false },
            State::Anchor { pos, .. } => Position { pos: Vec2::new(pos.0 as f64, pos.1 as f64), radius: None, held: false },
            State::Planet { pos, radius } => Position { pos: *pos, radius: Some(*radius), held: false },
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
            State::Planet { pos, radius } => {
                // only the neighbourhood of the last position is searched: a bright moon or a star elsewhere cannot take over
                let l = locate_planet(frame, Some((*pos, *radius))).ok_or(TrackError::NoDisc)?;
                *pos = l.pos;
                *radius = l.radius;
                Ok(Position { pos: l.pos, radius: Some(l.radius), held: false })
            }
            State::Anchor { pos, block } => {
                update_anchor(pos, block, frame)?;
                Ok(Position { pos: Vec2::new(pos.0 as f64, pos.1 as f64), radius: None, held: false })
            }
        }
    }
}

/// Edge points of the bright region, excluding points where the region is merely cut off by the frame border.
fn limb_points(frame: &Frame, expected: Option<f64>) -> Vec<(f64, f64)> {
    let st = DISC_STEP;
    let Some(view) = view_gain(frame) else { return vec![] };
    let grid = sample_grid(frame, view);
    let Some(thr) = sun_threshold(&grid) else { return vec![] };
    let margin = 2 * st;
    let mut pts = vec![];
    let gw = grid.gw;
    let mut y = margin;
    while y + margin < frame.height {
        let gy = (y / st) as usize;
        let (up, row, down) = (&grid.px[(gy - 1) * gw..gy * gw], &grid.px[gy * gw..(gy + 1) * gw], &grid.px[(gy + 1) * gw..(gy + 2) * gw]);
        let mut x = margin;
        while x + margin < frame.width {
            let gx = (x / st) as usize;
            if row[gx] >= thr && (row[gx - 1] < thr || row[gx + 1] < thr || up[gx] < thr || down[gx] < thr) {
                pts.push((x as f64, y as f64));
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
    let (xs, ys): (Vec<f64>, Vec<f64>) = pts.iter().copied().unzip();
    for _ in 0..300 {
        let (a, b, c) = (pts[next()], pts[next()], pts[next()]);
        let d = 2.0 * (a.0 * (b.1 - c.1) + b.0 * (c.1 - a.1) + c.0 * (a.1 - b.1));
        if d.abs() < 1e-6 { continue; }
        let ux = ((a.0 * a.0 + a.1 * a.1) * (b.1 - c.1) + (b.0 * b.0 + b.1 * b.1) * (c.1 - a.1) + (c.0 * c.0 + c.1 * c.1) * (a.1 - b.1)) / d;
        let uy = ((a.0 * a.0 + a.1 * a.1) * (c.0 - b.0) + (b.0 * b.0 + b.1 * b.1) * (a.0 - c.0) + (c.0 * c.0 + c.1 * c.1) * (b.0 - a.0)) / d;
        let r = (a.0 - ux).hypot(a.1 - uy);
        if !(20.0..=20000.0).contains(&r) || near.is_some_and(|e| (r / e - 1.0).abs() > 0.2) { continue; }
        // |d - r| <= tol  <=>  (r - tol)^2 <= d^2 <= (r + tol)^2 (r >= 20 > tol): no square root per point, and a loop the compiler vectorises
        let (lo, hi) = ((r - tol) * (r - tol), (r + tol) * (r + tol));
        let need = best.map_or(0, |b: (f64, f64, f64, usize)| b.3 + 1);
        let mut inl = 0usize;
        let mut beaten = false;
        for (i, (cx, cy)) in xs.chunks(256).zip(ys.chunks(256)).enumerate() {
            let mut c = 0u32;
            for (&x, &y) in cx.iter().zip(cy) {
                let d2 = (x - ux) * (x - ux) + (y - uy) * (y - uy);
                c += (d2 >= lo && d2 <= hi) as u32;
            }
            inl += c as usize;
            // the circle cannot overtake the best one any more: stop counting (same result, less work)
            if inl + (n - (i * 256 + cx.len())) < need { beaten = true; break; }
        }
        if !beaten && best.map_or(true, |b| inl > b.3) { best = Some((ux, uy, r, inl)); }
    }
    best
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
    fit_limb_pts(&limb_points(frame, radius), start, radius)
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
    let pts = limb_points(frame, expected);
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
    let pts = limb_points(frame, expected);
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

    /// A planet: a limb-darkened disc of radius `r` with soft edge, a 3 px moon nearby and noise, on a 16-bit sky.
    fn planet_frame(cx: f64, cy: f64, r: f64, seed: u32) -> Frame {
        let (w, h) = (1280u32, 960u32);
        let mut d = Vec::with_capacity((w * h * 2) as usize);
        let mut s = seed.wrapping_mul(2654435761).wrapping_add(1);
        for y in 0..h {
            for x in 0..w {
                s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                let noise = ((s >> 16) as f64 / 65536.0 - 0.5) * 500.0;
                let (dx, dy) = (x as f64 - cx, y as f64 - cy);
                let rr = (dx * dx + dy * dy).sqrt();
                let planet = if rr < r { 30000.0 * (1.0 - 0.4 * (rr / r).powi(2)) } else { 0.0 };
                let moon = (-(((dx - 90.0).powi(2) + (dy + 40.0).powi(2)) / 4.0)).exp() * 8000.0;
                let v = (1500.0 + planet + moon + noise).clamp(0.0, 65535.0) as u16;
                d.extend_from_slice(&v.to_le_bytes());
            }
        }
        Frame::new(w, h, PixelFormat::Mono16, d)
    }

    #[test]
    fn a_small_planet_is_found_and_followed_by_its_centroid() {
        let f = planet_frame(612.3, 470.8, 25.0, 1);
        let l = locate_planet(&f, None).expect("a planet is found");
        assert!((l.pos.x - 612.3).abs() < 0.7 && (l.pos.y - 470.8).abs() < 0.7, "{:?}", l.pos);
        assert!((l.radius - 22.0).abs() < 6.0, "radius {}", l.radius); // half maximum of a limb-darkened disc: a little inside the edge
        let mut t = Tracker::planet(&f).unwrap();
        assert_eq!(t.mode(), "planet");
        for (i, (x, y)) in [(615.0, 472.0), (618.5, 469.0), (609.0, 475.5)].into_iter().enumerate() {
            let p = t.update(&planet_frame(x, y, 25.0, i as u32 + 2)).unwrap();
            assert!((p.pos.x - x).abs() < 0.7 && (p.pos.y - y).abs() < 0.7, "{:?} vs {x},{y}", p.pos);
        }
    }

    #[test]
    fn the_planet_tracker_is_not_taken_by_a_moon_or_by_noise() {
        // the 3 px moon alone (no planet in view) is not a planet
        assert!(locate_planet(&planet_frame(-500.0, -500.0, 25.0, 3), None).is_none(), "a 3 px moon is smaller than a planet");
        // an empty sky
        let mut d = vec![0u8; 1280 * 960 * 2];
        for (i, px) in d.chunks_exact_mut(2).enumerate() { px.copy_from_slice(&((1500 + (i * 7919) % 400) as u16).to_le_bytes()); }
        assert!(locate_planet(&Frame::new(1280, 960, PixelFormat::Mono16, d), None).is_none());
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

    /// A 16-bit frame of a partial disc (sky ~0.8 in 8-bit units like the real camera's black level), scaled by `level`
    /// (1.0 = a bright 200-count disc), with an optional dark wedge and a dark vertical band across the disc: a tree's shadow.
    fn shadowed(cx: f64, cy: f64, r: f64, level: f64, shadows: bool) -> Frame {
        let (w, h) = (640u32, 480u32);
        let mut d = Vec::with_capacity((w * h * 2) as usize);
        for y in 0..h {
            for x in 0..w {
                let (dx, dy) = (x as f64 - cx, y as f64 - cy);
                let mut v = 0.8; // the sensor's black level, under everything
                if dx * dx + dy * dy < r * r {
                    let mut sig = 200.0 * level * (1.0 - 0.3 * (dx * dx + dy * dy).sqrt() / r); // limb darkening
                    if shadows {
                        if (x as f64 - 300.0).abs() < 25.0 { sig *= 0.08; }                                // a vertical band, nearly black
                        if (x as f64 - (400.0 + (y as f64 - 100.0) * 0.12)).abs() < 3.0 && y > 100 { sig = 0.0; } // thin slit down to sky level
                    }
                    v += sig;
                }
                d.extend_from_slice(&((v * 256.0 + 0.5) as u16).to_le_bytes());
            }
        }
        Frame::new(w, h, PixelFormat::Mono16, d)
    }

    #[test]
    fn limb_survives_shadows_and_a_hundredfold_dimmer_light() {
        let (cx, cy, r) = (520.0, 300.0, 380.0);
        for (level, shadows) in [(1.0, false), (1.0, true), (0.05, true), (0.012, true)] {
            let f = shadowed(cx, cy, r, level, shadows);
            let s = locate_sun(&f, Some(r)).unwrap_or_else(|| panic!("no Sun at level {level} shadows {shadows}"));
            assert!(s.partial);
            assert!((s.pos.x - cx).abs() < 10.0 && (s.pos.y - cy).abs() < 10.0, "level {level} shadows {shadows}: {:?}", s);
            assert!((s.radius - r).abs() < 15.0, "level {level} shadows {shadows}: radius {}", s.radius);
        }
        // and the limb tracker keeps following through it
        let mut t = Tracker::limb(&shadowed(cx, cy, r, 1.0, false), Some(r)).unwrap();
        let p = t.update(&shadowed(cx + 6.0, cy - 4.0, r, 0.02, true)).unwrap();
        assert!((p.pos.x - (cx + 6.0)).abs() < 10.0 && (p.pos.y - (cy - 4.0)).abs() < 10.0, "{:?}", p);
    }

    #[test]
    fn a_frame_below_the_noise_has_no_sun() {
        let f = shadowed(520.0, 300.0, 380.0, 0.0005, false);
        assert!(locate_sun(&f, Some(380.0)).is_none());
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

#[cfg(test)]
mod reference;
#[cfg(test)]
mod equiv;
