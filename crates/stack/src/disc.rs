//! The Sun's disc in a prominence-lane stack. The recorder saves those frames raw (the disc saturated, not blacked out) together with
//! where its Sun model put the disc in every frame. After stacking, the stacker works out where the disc is in the *stack*: first
//! from the recorded positions moved by each kept frame's alignment shift (median, so a frame or two that is off does not matter),
//! then refined by fitting a circle to the disc's edge in the stacked picture itself. Only then is a black disc drawn, if asked for.
//! Blacking out frame by frame at capture time jumped around with the model's error and left rings at the limb after alignment.

use crate::stack::Stacked;
use serde::{Deserialize, Serialize};

/// One recorded disc position: x and y of the centre and the radius, in pixels of the recorded frame. Radius 0 = unknown.
pub type Pos = [f32; 3];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DiscInfo {
    pub x: f64,
    pub y: f64,
    pub r: f64,
    /// "fitted" to the edge in the stack, or "recorded" (the positions from the recorder, when the edge could not be found).
    pub how: String,
    /// A black disc of `r * scale` was drawn.
    pub masked: bool,
    pub scale: f64,
}

/// The recorded positions of a recording, from its notes file (`details.disc_track`).
pub fn track_from_sidecar(recording: &std::path::Path) -> Option<Vec<Pos>> {
    let text = std::fs::read_to_string(recording.with_extension("json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    parse(v.get("details")?.get("disc_track")?)
}

pub fn parse(v: &serde_json::Value) -> Option<Vec<Pos>> {
    let a = v.as_array()?;
    let mut out = Vec::with_capacity(a.len());
    for p in a {
        let p = p.as_array()?;
        if p.len() != 3 {
            return None;
        }
        out.push([p[0].as_f64()? as f32, p[1].as_f64()? as f32, p[2].as_f64()? as f32]);
    }
    Some(out)
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 }
}

fn sample(d: &[f32], w: usize, h: usize, x: f64, y: f64) -> f64 {
    if x < 0.0 || y < 0.0 || x >= (w - 1) as f64 || y >= (h - 1) as f64 {
        return f64::NAN;
    }
    let (ix, iy, fx, fy) = (x.floor() as usize, y.floor() as usize, x.fract(), y.fract());
    let at = |xx: usize, yy: usize| d[yy * w + xx] as f64;
    at(ix, iy) * (1.0 - fx) * (1.0 - fy) + at(ix + 1, iy) * fx * (1.0 - fy) + at(ix, iy + 1) * (1.0 - fx) * fy + at(ix + 1, iy + 1) * fx * fy
}

/// Least-squares circle through points (Kåsa): (cx, cy, r).
fn circle(p: &[(f64, f64)]) -> Option<(f64, f64, f64)> {
    let n = p.len() as f64;
    let (mx, my) = (p.iter().map(|q| q.0).sum::<f64>() / n, p.iter().map(|q| q.1).sum::<f64>() / n);
    let (mut suu, mut suv, mut svv, mut suuu, mut svvv, mut suvv, mut svuu) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    for q in p {
        let (u, v) = (q.0 - mx, q.1 - my);
        suu += u * u;
        suv += u * v;
        svv += v * v;
        suuu += u * u * u;
        svvv += v * v * v;
        suvv += u * v * v;
        svuu += v * u * u;
    }
    let det = suu * svv - suv * suv;
    if det.abs() < 1e-9 {
        return None;
    }
    let (a, b) = ((suuu + suvv) / 2.0, (svvv + svuu) / 2.0);
    let (uc, vc) = ((a * svv - b * suv) / det, (b * suu - a * suv) / det);
    let r = (uc * uc + vc * vc + (suu + svv) / n).sqrt();
    Some((mx + uc, my + vc, r))
}

/// Finds the disc's edge near the guess and fits a circle to it. None when the picture shows no clear disc there.
fn fit_edge(d: &[f32], w: usize, h: usize, (cx, cy, r0): (f64, f64, f64)) -> Option<(f64, f64, f64, usize)> {
    // brightness inside the disc and just outside it
    let (mut inside, mut outside) = (vec![], vec![]);
    let step = (r0 / 40.0).max(2.0);
    let mut y = cy - 1.3 * r0;
    while y <= cy + 1.3 * r0 {
        let mut x = cx - 1.3 * r0;
        while x <= cx + 1.3 * r0 {
            let dist = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
            let v = sample(d, w, h, x, y);
            if v.is_finite() {
                if dist < 0.6 * r0 {
                    inside.push(v);
                } else if dist > 1.12 * r0 {
                    outside.push(v);
                }
            }
            x += step;
        }
        y += step;
    }
    if inside.len() < 50 || outside.len() < 50 {
        return None;
    }
    let (lo, hi) = (median(&mut outside), median(&mut inside));
    if hi < 1.5 * lo.max(1e-6) {
        return None; // no contrast: the disc is not distinguishable from the sky here
    }
    let t = lo + 0.5 * (hi - lo);
    let mut pts = vec![];
    for k in 0..360 {
        let a = (k as f64).to_radians();
        let (ca, sa) = (a.cos(), a.sin());
        let mut prev: Option<(f64, f64)> = None;
        let mut rr = 0.85 * r0;
        while rr <= 1.15 * r0 {
            let v = sample(d, w, h, cx + rr * ca, cy + rr * sa);
            if !v.is_finite() {
                break;
            }
            if let Some((pr, pv)) = prev {
                if pv >= t && v < t {
                    let f = (pv - t) / (pv - v);
                    pts.push((cx + (pr + f * (rr - pr)) * ca, cy + (pr + f * (rr - pr)) * sa));
                    break;
                }
            } else if v < t {
                break; // the ray starts outside the disc: it is not on this limb
            }
            prev = Some((rr, v));
            rr += 0.5;
        }
    }
    if pts.len() < 90 {
        return None;
    }
    // fit, drop outliers (prominences, cloud, the edge of the picture), fit again
    let mut keep = pts.clone();
    let mut c = circle(&keep)?;
    for _ in 0..5 {
        let res: Vec<f64> = pts.iter().map(|q| ((q.0 - c.0).powi(2) + (q.1 - c.1).powi(2)).sqrt() - c.2).collect();
        let med = median(&mut res.clone());
        let mad = median(&mut res.iter().map(|r| (r - med).abs()).collect::<Vec<_>>());
        let lim = (3.0 * 1.4826 * mad).max(1.5);
        keep = pts.iter().zip(&res).filter(|(_, r)| (**r - med).abs() <= lim).map(|(q, _)| *q).collect();
        if keep.len() < 90 {
            return None;
        }
        c = circle(&keep)?;
    }
    let ok = (c.2 - r0).abs() <= 0.12 * r0 && ((c.0 - cx).powi(2) + (c.1 - cy).powi(2)).sqrt() <= 0.15 * r0;
    ok.then_some((c.0, c.1, c.2, keep.len()))
}

/// Where the disc is in this stack. `track` holds the recorded position of every frame of the recording.
pub fn locate(s: &Stacked, track: &[Pos]) -> Result<DiscInfo, String> {
    // a frame's content lands in the stack displaced by minus its shift
    let (mut xs, mut ys, mut rs) = (vec![], vec![], vec![]);
    for &(i, dx, dy) in &s.report.shifts {
        if let Some(p) = track.get(i).filter(|p| p[2] > 0.0 && p[0].is_finite() && p[1].is_finite()) {
            xs.push(p[0] as f64 - dx);
            ys.push(p[1] as f64 - dy);
            rs.push(p[2] as f64);
        }
    }
    if xs.is_empty() {
        return Err("none of the kept frames has a recorded disc position (the Sun was not in view)".into());
    }
    let guess = (median(&mut xs), median(&mut ys), median(&mut rs));
    Ok(match fit_edge(&s.data, s.width, s.height, guess) {
        Some((x, y, r, _)) => DiscInfo { x, y, r, how: "fitted".into(), masked: false, scale: 1.0 },
        None => DiscInfo { x: guess.0, y: guess.1, r: guess.2, how: "recorded".into(), masked: false, scale: 1.0 },
    })
}

/// Black out the disc (`scale` times its radius) in the stack.
pub fn blank(s: &mut Stacked, d: &mut DiscInfo, scale: f64) {
    let (w, h) = (s.width, s.height);
    let r = d.r * scale;
    for y in ((d.y - r).floor().max(0.0) as usize)..=((d.y + r).ceil().min(h as f64 - 1.0) as usize) {
        let dy = y as f64 - d.y;
        let half = (r * r - dy * dy).max(0.0).sqrt();
        let x0 = (d.x - half).ceil().max(0.0) as usize;
        let x1 = (d.x + half).floor().min(w as f64 - 1.0);
        if x1 < 0.0 || x0 as f64 > x1 {
            continue;
        }
        s.data[y * w + x0..y * w + x1 as usize + 1].fill(0.0);
    }
    d.masked = true;
    d.scale = scale;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stack::Report;

    fn stacked(w: usize, h: usize, c: (f64, f64), r: f64) -> Stacked {
        let mut data = vec![0f32; w * h];
        for y in 0..h {
            for x in 0..w {
                let d = ((x as f64 - c.0).powi(2) + (y as f64 - c.1).powi(2)).sqrt();
                // a saturated disc with a soft edge, a faint corona, and a prominence above the limb
                let mut v: f64 = if d < r { 60000.0 } else { 3000.0 * (-(d - r) / 30.0).exp() };
                if d >= r && d < r + 1.5 {
                    v = 30000.0;
                }
                if (x as f64 - (c.0 + 0.6 * r)).abs() < 12.0 && (y as f64 - (c.1 - 1.08 * r)).abs() < 14.0 {
                    v = 50000.0;
                }
                data[y * w + x] = v as f32;
            }
        }
        let report = Report { frames: 10, kept: 3, score: vec![], kept_frames: vec![0, 1, 2], shifts: vec![(0, 0.0, 0.0), (1, 4.0, -3.0), (2, -5.0, 2.0)], shift_rms_px: 0.0, refine_change_px: 0.0, clipped_fraction: 0.0, signal: 0.0, signal_global: 0.0, aps: 0, local_shift_rms_px: 0.0, multipoint_seconds: 0.0, phases: vec![], first_timestamp: None, rank_seconds: 0.0, stack_seconds: 0.0, flat_applied: false, dark_applied: false };
        Stacked { width: w, height: h, data, report, disc: None }
    }

    #[test]
    fn the_disc_is_found_in_the_stack_even_when_the_recorded_positions_are_off() {
        let mut s = stacked(800, 640, (402.4, 318.7), 230.0);
        // recorded positions: each frame's disc centre = where it ended up plus the shift, with a model error of a few pixels
        let truth = (402.4, 318.7);
        let track: Vec<Pos> = vec![[(truth.0 + 0.0 + 7.0) as f32, (truth.1 + 0.0 - 6.0) as f32, 226.0], [(truth.0 + 4.0 + 7.0) as f32, (truth.1 - 3.0 - 6.0) as f32, 226.0], [(truth.0 - 5.0 + 7.0) as f32, (truth.1 + 2.0 - 6.0) as f32, 226.0]];
        let mut d = locate(&s, &track).unwrap();
        assert_eq!(d.how, "fitted");
        assert!((d.x - truth.0).abs() < 0.8 && (d.y - truth.1).abs() < 0.8 && (d.r - 230.0).abs() < 2.0, "{d:?}");
        blank(&mut s, &mut d, 1.03);
        assert_eq!(s.data[318 * 800 + 402], 0.0);
        assert!(s.data[318 * 800 + (402.0 + 230.0 * 1.03 + 4.0) as usize] > 0.0, "outside the disc is untouched");
        assert!(d.masked);
    }

    #[test]
    fn without_a_visible_disc_the_recorded_position_is_used_and_says_so() {
        let s = stacked(400, 300, (200.0, 150.0), 0.0001); // nothing there
        let track: Vec<Pos> = vec![[210.0, 140.0, 80.0]; 3];
        let d = locate(&s, &track).unwrap();
        assert_eq!(d.how, "recorded");
        assert!((d.x - 210.0).abs() < 4.5 && d.r == 80.0);
        assert!(locate(&s, &[[0.0, 0.0, 0.0]; 3]).is_err(), "no positions, no disc");
    }

    #[test]
    fn positions_parse_from_notes() {
        let v = serde_json::json!([[1.5, 2.5, 3.0], [4, 5, 6]]);
        assert_eq!(parse(&v).unwrap(), vec![[1.5, 2.5, 3.0], [4.0, 5.0, 6.0]]);
        assert!(parse(&serde_json::json!([[1, 2]])).is_none());
    }
}
