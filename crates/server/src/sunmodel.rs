//! Where the Sun is, as a prediction corrected by measurements, not a measurement alone.
//!
//! The detector sees one frame at a time: a cloud, a tree's shadow, an exposure change or a solar eclipse makes it jump
//! or lose the Sun for a while. Anything that has to stay put (the coronagraph mask above all) follows this model:
//! a smoothed position and radius plus a drift estimate, corrected only by measurements that agree with the
//! prediction. What is fought is mostly slow drift, so a measurement a small step from the prediction always counts
//! (more the smaller the step, even if cloud makes its radius wrong), and the drift velocity is the least-squares slope
//! of the last minute or so of accepted measurements, so it is not lost when clouds or a tree hide the limb. Disagreeing measurements are set aside; if they keep agreeing with each other for a while the Sun really
//! moved (a slew, a re-aim) and the model re-locks there. With no measurement at all it keeps predicting.

use rustflare_core::Vec2;
use rustflare_tracking::SunLocation;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// A measurement is believed if its centre is within this fraction of the radius of the prediction.
const GATE_RADII: f64 = 0.06;
/// ... and its radius within this fraction of the model's.
const GATE_RADIUS: f64 = 0.06;
/// Disagreeing measurements that agree with each other this closely (fraction of the radius) re-lock the model.
const RELOCK_SPREAD_RADII: f64 = 0.08;
const RELOCK_MIN_SAMPLES: usize = 8;
const RELOCK_MIN_SPAN: Duration = Duration::from_millis(400);
const PENDING_MAX_AGE: Duration = Duration::from_secs(15);
/// A step this small (fraction of the radius) from the prediction is drift, not noise: believed in full.
const SMALL_STEP_RADII: f64 = 0.015;
/// How much of the last minutes of measurements the drift estimate looks at, and the least it needs.
const HISTORY_SPAN: Duration = Duration::from_secs(90);
const HISTORY_MIN_SPAN: Duration = Duration::from_secs(30); // a shorter baseline gives a slope dominated by noise
const HISTORY_MIN_SAMPLES: usize = 8;
/// Time constants of the smoothing of the centre and the radius, seconds.
const TAU_POS: f64 = 3.0; // the real drift is about a pixel a second: smooth hard, the drift term removes the lag
const TAU_RADIUS: f64 = 20.0;
/// Drift (px/s) is learned slowly and capped: the mount tracks the Sun, what is left is small.
const MAX_DRIFT: f64 = 6.0; // the real drift is 0.2 to 4 px/s: anything bigger is a bad estimate
/// While predicting, the estimate never moves further than this fraction of the radius from the last accepted measurement.
const MAX_EXTRAPOLATION_RADII: f64 = 0.35;
/// Measured radii further than this from the optics' radius are not believed.
const RADIUS_PRIOR_TOL: f64 = 0.06;

/// What the model says for the frame just seen.
#[derive(Copy, Clone, Debug)]
pub struct Estimate {
    pub sun: SunLocation,
    /// True when this frame's measurement was not used (none, or it disagreed): the position is a prediction.
    pub predicted: bool,
    /// Seconds since a measurement last agreed with the model.
    pub since_measured_s: f64,
}

#[derive(Default)]
pub struct SunModel {
    state: Option<State>,
    pending: Vec<(Instant, SunLocation)>,
    /// Accepted measurements (about two a second) for the drift slope.
    history: VecDeque<(Instant, Vec2)>,
    /// Long-term drift (px/s) learned while the images were good: the starting guess after a re-lock, and what keeps
    /// the prediction honest when clouds or an eclipse hide the Sun for long. Turns over with the image at a meridian flip.
    learned: Option<Vec2>,
}

#[derive(Copy, Clone)]
struct State {
    pos: Vec2,
    radius: f64,
    vel: Vec2,
    at: Instant,
    last_measured: Instant,
    /// Where the Sun was when a measurement last agreed: predictions stay near it.
    anchor: Vec2,
    partial: bool,
    dims: (u32, u32),
    /// Measurements that have agreed with the model: a young model (started on one frame, maybe a bad one) is easy to correct.
    confirmed: u32,
}

impl SunModel {
    /// The image turned over (meridian flip): the drift turns with it.
    pub fn flipped(&mut self) {
        if let Some(l) = self.learned.as_mut() {
            *l = *l * -1.0;
        }
        if let Some(s) = self.state.as_mut() {
            s.vel = s.vel * -1.0;
        }
        self.history.clear();
    }

    /// The long-term drift learned so far, px/s.
    pub fn learned_drift(&self) -> Option<Vec2> {
        self.learned
    }

    pub fn reset(&mut self) {
        self.state = None;
        self.pending.clear();
        self.history.clear();
    }

    /// The model's current estimate without feeding it anything.
    pub fn current(&self) -> Option<SunLocation> {
        self.state.map(|s| SunLocation { pos: s.pos, radius: s.radius, partial: s.partial, radius_from_optics: None, fit_rms: None })
    }

    /// Moves the estimate by `d` pixels: a relative displacement seen in frames that cannot give a position of their own
    /// (the long exposures of a lane recording, whose saturated disc is biased but moves with the Sun).
    pub fn shift(&mut self, d: Vec2) {
        if let Some(s) = self.state.as_mut() {
            s.pos = s.pos + d;
        }
    }

    /// Feeds the detector's result for one frame (`None`: nothing found) and returns the model's estimate.
    /// `free`: the mount is slewing (or being moved by hand): the Sun really moves, so follow the measurements directly.
    pub fn update(&mut self, meas: Option<SunLocation>, now: Instant, dims: (u32, u32), free: bool, radius_prior: Option<f64>) -> Option<Estimate> {
        if self.state.is_some_and(|s| s.dims != dims) {
            self.reset();
        }
        let Some(mut s) = self.state else {
            // nothing to predict from yet: the first measurement starts the model
            let m = meas?;
            self.state = Some(State { pos: m.pos, radius: m.radius, vel: self.learned.unwrap_or(Vec2::new(0.0, 0.0)), at: now, last_measured: now, anchor: m.pos, partial: m.partial, dims, confirmed: 0 });
            return Some(Estimate { sun: m, predicted: false, since_measured_s: 0.0 });
        };
        // predict forward, but never far from the last place a measurement agreed
        let dt = now.saturating_duration_since(s.at).as_secs_f64().min(600.0);
        s.pos = s.pos + s.vel * dt;
        let off = s.pos - s.anchor;
        if !free && off.len() > MAX_EXTRAPOLATION_RADII * s.radius {
            s.pos = s.anchor + off * (MAX_EXTRAPOLATION_RADII * s.radius / off.len());
        }
        s.at = now;
        let mut predicted = true;
        if let Some(m) = meas {
            let step = (m.pos - s.pos).len();
            let radius_ok = (m.radius / s.radius - 1.0).abs() <= GATE_RADIUS && radius_prior.map_or(true, |p| (m.radius / p - 1.0).abs() <= RADIUS_PRIOR_TOL);
            let small = SMALL_STEP_RADII * s.radius;
            let gate = GATE_RADII * s.radius;
            // weight of this measurement: full for a drift-sized step, falling off towards the gate, and low when only the
            // centre can be trusted (the radius is off: cloud thinning the limb)
            let weight = if free {
                1.0
            } else if step <= gate && radius_ok {
                if step <= small { 1.0 } else { 1.0 - 0.9 * (step - small) / (gate - small) }
            } else if step <= 2.0 * small {
                0.3
            } else {
                0.0
            };
            if weight > 0.0 {
                let a = (1.0 - (-dt / TAU_POS).exp()) * weight;
                if free {
                    s.pos = m.pos;
                    s.anchor = m.pos;
                    s.vel = Vec2::new(0.0, 0.0);
                    s.radius = m.radius;
                    self.history.clear();
                } else {
                    s.pos = s.pos + (m.pos - s.pos) * a;
                    if radius_ok {
                        s.radius += (m.radius - s.radius) * (1.0 - (-dt / TAU_RADIUS).exp()) * weight;
                    }
                    // the centre alone is good for the drift too (cloud can spoil the radius and leave the centre fine)
                    if weight >= 0.3 && self.history.back().map_or(true, |(t, _)| now.saturating_duration_since(*t) >= Duration::from_millis(500)) {
                        self.history.push_back((now, m.pos));
                    }
                }
                while self.history.front().is_some_and(|(t, _)| now.saturating_duration_since(*t) > HISTORY_SPAN) {
                    self.history.pop_front();
                }
                if let Some(v) = drift_slope(&self.history) {
                    s.vel = if v.len() > MAX_DRIFT { v * (MAX_DRIFT / v.len()) } else { v };
                    // a long, clean stretch teaches the long-term drift slowly
                    if self.history.back().zip(self.history.front()).is_some_and(|(b, f)| b.0.saturating_duration_since(f.0) >= Duration::from_secs(40)) {
                        self.learned = Some(match self.learned { Some(l) => l * 0.98 + s.vel * 0.02, None => s.vel });
                    }
                }
                s.partial = m.partial;
                s.last_measured = now;
                s.anchor = s.pos;
                s.confirmed = s.confirmed.saturating_add(1);
                predicted = false;
                self.pending.clear();
            } else {
                self.pending.push((now, m));
            }
        }
        self.pending.retain(|(t, _)| now.saturating_duration_since(*t) <= PENDING_MAX_AGE);
        if self.pending.len() > 200 {
            self.pending.drain(..self.pending.len() - 200);
        }
        if self.pending.len() >= RELOCK_MIN_SAMPLES {
            // the biggest group of disagreeing measurements that agree with each other (shadow circles are all different,
            // a Sun that really moved is the same place again and again)
            // after a long time without any agreeing measurement the model is probably wrong: accept a looser cluster
            let spread = if now.saturating_duration_since(s.last_measured) > Duration::from_secs(20) { 2.0 * RELOCK_SPREAD_RADII } else { RELOCK_SPREAD_RADII };
            let close = |a: &SunLocation, b: &SunLocation| (a.pos - b.pos).len() <= spread * a.radius.max(b.radius);
            let (mut best, mut best_n) = (0usize, 0usize);
            for (i, (_, a)) in self.pending.iter().enumerate() {
                let n = self.pending.iter().filter(|(_, b)| close(a, b)).count();
                if n > best_n { best = i; best_n = n; }
            }
            if best_n >= RELOCK_MIN_SAMPLES {
                let anchor = self.pending[best].1;
                let group: Vec<&(Instant, SunLocation)> = self.pending.iter().filter(|(_, b)| close(&anchor, b)).collect();
                let n = group.len() as f64;
                let mean = group.iter().fold(Vec2::new(0.0, 0.0), |acc, (_, m)| acc + m.pos) * (1.0 / n);
                let r = group.iter().map(|(_, m)| m.radius).sum::<f64>() / n;
                let span = group.last().unwrap().0.saturating_duration_since(group[0].0);
                if span >= RELOCK_MIN_SPAN && (s.confirmed < 40 || (r / s.radius - 1.0).abs() <= 0.1) {
                    let partial = group.last().unwrap().1.partial;
                    s.pos = mean;
                    s.anchor = mean;
                    s.radius = r;
                    s.vel = self.learned.unwrap_or(Vec2::new(0.0, 0.0));
                    self.history.clear();
                    s.confirmed = 0;
                    s.last_measured = now;
                    s.partial = partial;
                    predicted = false;
                    self.pending.clear();
                }
            }
        }
        if let Some(p) = radius_prior {
            s.radius = s.radius.clamp(p * (1.0 - RADIUS_PRIOR_TOL), p * (1.0 + RADIUS_PRIOR_TOL));
        }
        self.state = Some(s);
        let since = now.saturating_duration_since(s.last_measured).as_secs_f64();
        Some(Estimate {
            sun: SunLocation { pos: s.pos, radius: s.radius, partial: s.partial, radius_from_optics: None, fit_rms: None },
            predicted,
            since_measured_s: since,
        })
    }
}

/// Robust velocity (px/s) of the accepted measurements, if they span enough time: the median of the slopes between pairs
/// of samples (Theil-Sen), so a few bad measurements cannot produce a wild drift.
fn drift_slope(h: &VecDeque<(Instant, Vec2)>) -> Option<Vec2> {
    if h.len() < HISTORY_MIN_SAMPLES {
        return None;
    }
    let t0 = h[0].0;
    if h.back()?.0.saturating_duration_since(t0) < HISTORY_MIN_SPAN {
        return None;
    }
    let step = (h.len() / 60).max(1);
    let pts: Vec<(f64, Vec2)> = h.iter().step_by(step).map(|(t, p)| (t.saturating_duration_since(t0).as_secs_f64(), *p)).collect();
    let (mut sx, mut sy) = (Vec::new(), Vec::new());
    for i in 0..pts.len() {
        for j in i + 1..pts.len() {
            let dt = pts[j].0 - pts[i].0;
            if dt >= 5.0 {
                sx.push((pts[j].1.x - pts[i].1.x) / dt);
                sy.push((pts[j].1.y - pts[i].1.y) / dt);
            }
        }
    }
    if sx.len() < 10 {
        return None;
    }
    let med = |v: &mut Vec<f64>| { v.sort_by(|a, b| a.total_cmp(b)); v[v.len() / 2] };
    Some(Vec2::new(med(&mut sx), med(&mut sy)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loc(x: f64, y: f64, r: f64) -> SunLocation {
        SunLocation { pos: Vec2::new(x, y), radius: r, partial: true, radius_from_optics: None, fit_rms: None }
    }
    const D: (u32, u32) = (1944, 1472);

    /// Runs `n` frames at 25 fps from `t0`, measurements from `f(i)`, returns the last estimate.
    fn run(m: &mut SunModel, t0: Instant, from: usize, n: usize, f: impl Fn(usize) -> Option<SunLocation>) -> Estimate {
        let mut last = None;
        for i in from..from + n {
            last = m.update(f(i), t0 + Duration::from_millis(40 * i as u64), D, false, None);
        }
        last.unwrap()
    }

    #[test]
    fn smooths_jitter() {
        let (mut m, t0) = (SunModel::default(), Instant::now());
        let e = run(&mut m, t0, 0, 600, |i| Some(loc(1000.0 + if i % 2 == 0 { 30.0 } else { -30.0 }, 700.0, 1200.0)));
        assert!((e.sun.pos.x - 1000.0).abs() < 12.0, "{:?}", e.sun.pos);
    }

    #[test]
    fn wild_measurements_are_ignored() {
        let (mut m, t0) = (SunModel::default(), Instant::now());
        run(&mut m, t0, 0, 50, |_| Some(loc(1000.0, 700.0, 1200.0)));
        // shadows: circles far off, of every size, a different one each frame
        let e = run(&mut m, t0, 50, 30, |i| Some(loc(300.0 + 40.0 * i as f64 % 900.0, 100.0 + 13.0 * i as f64 % 500.0, 500.0 + 11.0 * (i % 7) as f64)));
        assert!(e.predicted);
        assert!((e.sun.pos.x - 1000.0).abs() < 5.0 && (e.sun.pos.y - 700.0).abs() < 5.0 && (e.sun.radius - 1200.0).abs() < 5.0, "{:?}", e.sun);
    }

    #[test]
    fn keeps_predicting_through_a_long_dropout_like_totality() {
        let (mut m, t0) = (SunModel::default(), Instant::now());
        // drifting 1 px/s (25 fps: 0.04 px per frame) for 60 s, learned
        run(&mut m, t0, 0, 1500, |i| Some(loc(1000.0 + 0.04 * i as f64, 700.0, 1200.0)));
        // then no detection for 5 minutes (7500 frames)
        let e = run(&mut m, t0, 1500, 7500, |_| None);
        assert!(e.predicted && e.since_measured_s > 290.0);
        let expected_x = 1000.0 + 0.04 * 9000.0;
        assert!((e.sun.pos.x - expected_x).abs() < 0.1 * 1200.0, "x {} expected about {}", e.sun.pos.x, expected_x);
        assert!((e.sun.radius - 1200.0).abs() < 1.0);
    }

    #[test]
    fn a_runaway_drift_cannot_carry_the_estimate_away() {
        // seen on the real Sun: cloud spoiled the measurements, the drift estimate went to its cap, and the held estimate
        // slid 1100 px away in under a minute. The velocity is capped at 6 px/s and the estimate stays near its anchor.
        let (mut m, t0) = (SunModel::default(), Instant::now());
        // a steady 8 px/s measured for 60 s (more than any real drift): the model's velocity is capped
        run(&mut m, t0, 0, 1500, |i| Some(loc(1000.0 + 0.32 * i as f64, 700.0, 1200.0)));
        let anchor_x = 1000.0 + 0.32 * 1499.0;
        let e = run(&mut m, t0, 1500, 15000, |_| None); // ten minutes of nothing
        assert!(e.predicted);
        assert!((e.sun.pos - Vec2::new(anchor_x, 700.0)).len() <= 0.36 * 1200.0, "moved {} px from the last good place", (e.sun.pos - Vec2::new(anchor_x, 700.0)).len());
    }

    #[test]
    fn the_radius_stays_near_the_optics_radius() {
        let (mut m, t0) = (SunModel::default(), Instant::now());
        let mut last = None;
        for i in 0..3000 {
            // measurements that read the radius 15 % small (hazy limb) for two minutes
            last = m.update(Some(loc(1000.0, 700.0, 1020.0)), t0 + Duration::from_millis(40 * i), D, false, Some(1200.0));
        }
        assert!(last.unwrap().sun.radius >= 1200.0 * 0.94 - 1.0, "radius {}", last.unwrap().sun.radius);
    }

    #[test]
    fn follows_a_real_move_once_measurements_agree_with_each_other() {
        let (mut m, t0) = (SunModel::default(), Instant::now());
        run(&mut m, t0, 0, 50, |_| Some(loc(1000.0, 700.0, 1200.0)));
        let e = run(&mut m, t0, 50, 60, |_| Some(loc(1500.0, 900.0, 1190.0))); // re-aimed: steady new position
        assert!(!e.predicted || (e.sun.pos.x - 1500.0).abs() < 20.0, "{:?}", e.sun);
        assert!((e.sun.pos.x - 1500.0).abs() < 20.0 && (e.sun.pos.y - 900.0).abs() < 20.0, "{:?}", e.sun);
    }

    #[test]
    fn follows_directly_while_the_mount_slews() {
        let (mut m, t0) = (SunModel::default(), Instant::now());
        run(&mut m, t0, 0, 10, |_| Some(loc(1000.0, 700.0, 1200.0)));
        let mut e = None;
        for i in 10..40 {
            e = m.update(Some(loc(1000.0 + 20.0 * i as f64, 700.0, 1200.0)), t0 + Duration::from_millis(40 * i as u64), D, true, None);
        }
        assert!((e.unwrap().sun.pos.x - (1000.0 + 20.0 * 39.0)).abs() < 1.0);
    }

    #[test]
    fn starts_without_a_measurement_only_when_it_has_one() {
        let mut m = SunModel::default();
        assert!(m.update(None, Instant::now(), D, false, None).is_none());
    }

    /// Deterministic noise in -1..1.
    fn noise(i: usize, k: u64) -> f64 {
        let mut x = (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ k.wrapping_mul(0xD6E8_FEB8_6659_FD93);
        x ^= x >> 29; x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9); x ^= x >> 32;
        (x % 20001) as f64 / 10000.0 - 1.0
    }

    #[test]
    fn learns_drift_through_noise_and_shadows_and_carries_it_through_a_dropout() {
        let (mut m, t0) = (SunModel::default(), Instant::now());
        let truth = |i: usize| 1000.0 + 0.12 * i as f64; // 3 px/s at 25 fps
        // 100 s: noisy circles (+-15 px, radius +-4 %), one frame in five a wild shadow circle
        let e = run(&mut m, t0, 0, 2500, |i| {
            if i % 5 == 0 { return Some(loc(300.0 + 700.0 * noise(i, 3).abs(), 200.0 + 500.0 * noise(i, 4).abs(), 600.0 + 400.0 * noise(i, 5).abs())); }
            Some(loc(truth(i) + 15.0 * noise(i, 1), 700.0 + 15.0 * noise(i, 2), 1200.0 * (1.0 + 0.04 * noise(i, 6))))
        });
        assert!((e.sun.pos.x - truth(2499)).abs() < 8.0, "x {} vs {}", e.sun.pos.x, truth(2499));
        assert!((e.sun.pos.y - 700.0).abs() < 8.0, "y {}", e.sun.pos.y);
        // then two minutes of nothing (cloud, trees): the drift keeps being applied
        let e = run(&mut m, t0, 2500, 3000, |_| None);
        assert!(e.predicted);
        assert!((e.sun.pos.x - truth(5499)).abs() < 25.0, "x {} vs {}", e.sun.pos.x, truth(5499));
    }

    #[test]
    fn a_small_step_counts_even_when_the_radius_is_wrong() {
        let (mut m, t0) = (SunModel::default(), Instant::now());
        run(&mut m, t0, 0, 50, |_| Some(loc(1000.0, 700.0, 1200.0)));
        // thin cloud: the radius reads 25 % small, but the centre follows a slow drift (3 px/s)
        let e = run(&mut m, t0, 50, 400, |i| Some(loc(1000.0 + 0.12 * (i - 50) as f64, 700.0, 900.0)));
        assert!((e.sun.pos.x - (1000.0 + 0.12 * 399.0)).abs() < 35.0, "{:?}", e.sun.pos);
        assert!((e.sun.radius - 1200.0).abs() < 1.0, "the wrong radius must not be adopted: {}", e.sun.radius);
    }

    #[test]
    fn long_term_drift_survives_a_relock_and_turns_over_with_a_flip() {
        let (mut m, t0) = (SunModel::default(), Instant::now());
        // two minutes of good images drifting 3 px/s teach the long-term drift
        run(&mut m, t0, 0, 3000, |i| Some(loc(1000.0 + 0.12 * i as f64, 700.0, 1200.0)));
        let d = m.learned_drift().expect("learned");
        assert!((d.x - 3.0).abs() < 0.5 && d.y.abs() < 0.5, "{d:?}");
        // the camera restarts: the first frame starts a new model, which already knows how it drifts
        m.reset();
        let e = m.update(Some(loc(1500.0, 700.0, 1200.0)), t0 + Duration::from_secs(130), D, false, None).unwrap();
        // 60 s of nothing: it moved on by about 3 px/s
        let e2 = m.update(None, t0 + Duration::from_secs(190), D, false, None).unwrap();
        assert!((e2.sun.pos.x - e.sun.pos.x - 180.0).abs() < 40.0, "{} -> {}", e.sun.pos.x, e2.sun.pos.x);
        // after a meridian flip the image is turned over: the drift is the other way
        m.flipped();
        assert!(m.learned_drift().unwrap().x < -2.0);
    }
}
