//! Calibration and guiding. The image-to-axes mapping and direction maths are
//! ported from Vidoxide's `guiding.rs`; the timing logic is restructured as
//! pure state machines driven by the caller.

use serde::Serialize;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use rustflare_core::{Mat2, Vec2};

#[derive(Copy, Clone, Debug, PartialEq, Serialize)]
pub struct Calibration {
    /// Image-space unit vector of positive primary-axis motion.
    pub primary_dir: Vec2,
    /// Image-space unit vector of positive secondary-axis motion.
    pub secondary_dir: Vec2,
    pub img_to_axes: Mat2,
    /// Measured response: image pixels moved per second at 1x sidereal guide rate, per axis.
    /// This is what lets the guider size its pulses for any pixel scale.
    pub rate: [f64; 2],
}

impl Calibration {
    /// Builds the calibration from the image displacement seen while each axis was guided at `speed`
    /// (multiples of sidereal) for `secs` seconds. Fails if the directions are (nearly) parallel.
    pub fn from_motion(primary: Vec2, secondary: Vec2, secs: f64, speed: f64) -> Option<Calibration> {
        let p = primary.normalized()?;
        let s = secondary.normalized()?;
        if (p.x * s.y - p.y * s.x).abs() < 0.2 {
            return None;
        }
        let per = secs * speed;
        Some(Calibration {
            primary_dir: p,
            secondary_dir: s,
            img_to_axes: Mat2::from_cols(p, s).invert()?,
            rate: [primary.len() / per, secondary.len() / per],
        })
    }

    /// Unit vector in axes space that moves the image along `image_offset`.
    pub fn guiding_direction(&self, image_offset: Vec2) -> Option<Vec2> {
        self.img_to_axes.apply(image_offset).normalized()
    }
}

/// Guide speeds in multiples of the sidereal rate, signed, per axis.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct GuideCommand {
    pub primary: f64,
    pub secondary: f64,
}

impl GuideCommand {
    pub const STOP: GuideCommand = GuideCommand { primary: 0.0, secondary: 0.0 };
}

/// The guider corrects the mount's *drift*, not the seeing: it averages the measured position over a window
/// (so atmospheric jitter cancels), ignores offsets inside a dead band, and sends small, gentle pulses, then
/// waits for a fresh average before judging again.
#[derive(Copy, Clone, Debug)]
pub struct GuideParams {
    /// Averaging window for the position estimate.
    pub window: Duration,
    /// Averaged offsets smaller than this (pixels) are left alone.
    pub deadband_px: f64,
    /// Fraction of the averaged error corrected per pulse (< 1: never over-correct).
    pub gain: f64,
    /// Guide rate for pulses, multiples of sidereal (gentle; the AM3 accepts 0.1-0.9).
    pub speed: f64,
    /// Faster rate (and longer pulses) used only while the averaged error exceeds `fast_above_px`,
    /// e.g. after a manual slew, so large offsets are not corrected at the gentle rate.
    pub fast_speed: f64,
    pub fast_above_px: f64,
    /// Pause after a correction before the next judgement, so the window refills with post-pulse positions.
    pub min_interval: Duration,
    pub max_pulse: Duration,
}

impl Default for GuideParams {
    fn default() -> Self {
        GuideParams {
            window: Duration::from_secs(10),
            deadband_px: 3.0,
            gain: 0.7,
            speed: 0.2,
            fast_speed: 0.8,
            fast_above_px: 20.0,
            min_interval: Duration::from_secs(6),
            max_pulse: Duration::from_millis(1500),
        }
    }
}

/// Keeps the tracked position on `target` by correcting slow drift.
pub struct Guider {
    pub target: Vec2,
    cal: Calibration,
    params: GuideParams,
    samples: VecDeque<(Instant, Vec2)>,
    /// Pulses still to send after the current one (axis by axis: the mount has one guide rate).
    pending: VecDeque<(GuideCommand, Duration)>,
    pulse_end: Option<Instant>,
    judge_after: Option<Instant>,
    filtered_error: Option<Vec2>,
    /// The last correction was large, so the error dwarfs any seeing: re-check quickly instead of averaging for long.
    quick: bool,
}

impl Guider {
    pub fn new(target: Vec2, cal: Calibration, params: GuideParams) -> Guider {
        Guider { target, cal, params, samples: VecDeque::new(), pending: VecDeque::new(), pulse_end: None, judge_after: None, filtered_error: None, quick: false }
    }

    /// Offset of the *averaged* position from the target (what the guider acts on), once available.
    pub fn filtered_error(&self) -> Option<Vec2> {
        self.filtered_error
    }

    /// Call on every new measurement; returns a command only when the mount needs one.
    pub fn step(&mut self, now: Instant, pos: Vec2) -> Option<GuideCommand> {
        // 1. a pulse is running (or just finished)
        if let Some(end) = self.pulse_end {
            if now < end {
                return None;
            }
            return match self.pending.pop_front() {
                Some((cmd, dur)) => {
                    self.pulse_end = Some(now + dur);
                    Some(cmd)
                }
                None => {
                    self.pulse_end = None;
                    self.samples.clear(); // positions measured while moving say nothing about the drift
                    self.judge_after = Some(now + if self.quick { Duration::from_secs(2) } else { self.params.min_interval });
                    Some(GuideCommand::STOP)
                }
            };
        }
        // 2. gather the averaging window
        self.samples.push_back((now, pos));
        while self.samples.front().is_some_and(|(t, _)| now.duration_since(*t) > self.params.window) {
            self.samples.pop_front();
        }
        if self.judge_after.is_some_and(|t| now < t) {
            return None;
        }
        let span = now.duration_since(self.samples.front()?.0);
        let needed = if self.quick { Duration::from_millis(1500) } else { self.params.window / 2 };
        if span < needed {
            return None; // not enough data to tell drift from seeing
        }
        let n = self.samples.len() as f64;
        let mean = self.samples.iter().fold(Vec2::default(), |a, (_, p)| a + *p) * (1.0 / n);
        let err = self.target - mean;
        self.filtered_error = Some(err);
        if err.len() < self.params.deadband_px {
            self.quick = false;
            return None;
        }
        // 3. plan gentle pulses, one axis at a time: image motion wanted = gain x error, in axis coordinates
        let big = err.len() > self.params.fast_above_px;
        self.quick = big;
        let (speed, max_pulse) = if big { (self.params.fast_speed, self.params.max_pulse * 2) } else { (self.params.speed, self.params.max_pulse) };
        let a = self.cal.img_to_axes.apply(err * self.params.gain);
        for (i, px) in [a.x, a.y].into_iter().enumerate() {
            let rate = self.cal.rate[i] * speed; // image px per second while pulsing
            if rate <= 0.0 {
                continue;
            }
            let dur = Duration::from_secs_f64((px.abs() / rate).min(max_pulse.as_secs_f64()));
            if dur < Duration::from_millis(100) {
                continue; // below what the 10 Hz measurement loop can time
            }
            let v = if px > 0.0 { speed } else { -speed };
            let cmd = if i == 0 { GuideCommand { primary: v, secondary: 0.0 } } else { GuideCommand { primary: 0.0, secondary: v } };
            self.pending.push_back((cmd, dur));
        }
        let (cmd, dur) = self.pending.pop_front()?;
        self.pulse_end = Some(now + dur);
        self.samples.clear();
        Some(cmd)
    }
}

#[derive(Debug, PartialEq)]
pub enum CalAction {
    None,
    Guide(GuideCommand),
    Done(Calibration),
    Failed(String),
}

enum Phase {
    Init,
    Moving { axis: usize, since: Instant, from: Vec2 },
    Settling { axis: usize, since: Instant, from: Vec2 },
}

/// Moves each axis in turn and measures the image displacement to learn the axis directions.
pub struct Calibrator {
    phase: Phase,
    motion: [Option<Vec2>; 2],
    pub move_time: Duration,
    pub settle_time: Duration,
    pub speed: f64,
    pub min_move_px: f64,
}

impl Calibrator {
    pub fn new() -> Calibrator {
        Calibrator {
            phase: Phase::Init,
            motion: [None; 2],
            move_time: Duration::from_secs(4),
            settle_time: Duration::from_secs(1),
            speed: 0.5,
            min_move_px: 5.0,
        }
    }

    fn cmd(&self, axis: usize) -> GuideCommand {
        if axis == 0 { GuideCommand { primary: self.speed, secondary: 0.0 } } else { GuideCommand { primary: 0.0, secondary: self.speed } }
    }

    pub fn step(&mut self, now: Instant, pos: Vec2) -> CalAction {
        match self.phase {
            Phase::Init => {
                self.phase = Phase::Moving { axis: 0, since: now, from: pos };
                CalAction::Guide(self.cmd(0))
            }
            Phase::Moving { axis, since, from } => {
                if now.duration_since(since) >= self.move_time {
                    self.phase = Phase::Settling { axis, since: now, from };
                    CalAction::Guide(GuideCommand::STOP)
                } else {
                    CalAction::None
                }
            }
            Phase::Settling { axis, since, from } => {
                if now.duration_since(since) < self.settle_time {
                    return CalAction::None;
                }
                let d = pos - from;
                if d.len() < self.min_move_px {
                    return CalAction::Failed(format!(
                        "image moved only {:.1}px on axis {} (is sky tracking on and the guide rate adequate?)",
                        d.len(), axis + 1
                    ));
                }
                self.motion[axis] = Some(d);
                if axis == 0 {
                    self.phase = Phase::Moving { axis: 1, since: now, from: pos };
                    CalAction::Guide(self.cmd(1))
                } else {
                    match Calibration::from_motion(self.motion[0].unwrap(), self.motion[1].unwrap(), self.move_time.as_secs_f64(), self.speed) {
                        Some(c) => CalAction::Done(c),
                        None => CalAction::Failed("axis directions are parallel; calibration unusable".into()),
                    }
                }
            }
        }
    }
}

impl Default for Calibrator {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustflare_core::sim::{axis_dirs, PX_PER_SIDEREAL, TRACKING_DRIFT};

    /// Fake world: the true image position integrates drift + commanded guide motion in 100 ms steps;
    /// measurements add seeing jitter (deterministic pseudo-random, uniform within +-`jitter` px).
    struct World { t0: Instant, now: Instant, pos: Vec2, cmd: GuideCommand, drift: Vec2, jitter: f64, rng: u64 }
    impl World {
        fn new(drift: Vec2, jitter: f64) -> World {
            let t = Instant::now();
            World { t0: t, now: t, pos: Vec2::new(640.0, 480.0), cmd: GuideCommand::STOP, drift, jitter, rng: 0x2545F4914F6CDD1D }
        }
        fn noise(&mut self) -> f64 {
            self.rng ^= self.rng << 13; self.rng ^= self.rng >> 7; self.rng ^= self.rng << 17;
            ((self.rng >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0) * self.jitter
        }
        fn tick(&mut self) {
            let (p, q) = axis_dirs();
            self.pos = self.pos + (self.drift + p * (self.cmd.primary * PX_PER_SIDEREAL) + q * (self.cmd.secondary * PX_PER_SIDEREAL)) * 0.1;
            self.now += Duration::from_millis(100);
        }
        /// What the tracker would report: the true position plus seeing jitter.
        fn measured(&mut self) -> Vec2 { let (nx, ny) = (self.noise(), self.noise()); self.pos + Vec2::new(nx, ny) }
        fn elapsed(&self) -> f64 { self.now.duration_since(self.t0).as_secs_f64() }
    }

    fn calibrate(w: &mut World) -> Calibration {
        let mut c = Calibrator::new();
        for _ in 0..1000 {
            let m = w.measured();
            match c.step(w.now, m) {
                CalAction::Guide(g) => w.cmd = g,
                CalAction::Done(cal) => { w.cmd = GuideCommand::STOP; return cal; }
                CalAction::Failed(e) => panic!("{e}"),
                CalAction::None => {}
            }
            w.tick();
        }
        panic!("calibration never finished");
    }

    /// Runs the guider for `secs`; returns (pulses sent, worst *true* error in the last `tail` seconds).
    fn run(w: &mut World, g: &mut Guider, secs: f64, tail: f64) -> (usize, f64) {
        let (start, mut pulses, mut worst) = (w.elapsed(), 0usize, 0f64);
        while w.elapsed() - start < secs {
            let m = w.measured();
            if let Some(c) = g.step(w.now, m) {
                if c != GuideCommand::STOP { pulses += 1; }
                w.cmd = c;
            }
            w.tick();
            if secs - (w.elapsed() - start) < tail {
                worst = worst.max((w.pos - g.target).len());
            }
        }
        (pulses, worst)
    }

    #[test]
    fn calibration_discovers_axis_directions_and_speed() {
        let mut w = World::new(TRACKING_DRIFT, 0.0);
        let cal = calibrate(&mut w);
        let (p, q) = axis_dirs();
        assert!((cal.primary_dir.x - p.x).abs() < 0.1 && (cal.primary_dir.y - p.y).abs() < 0.1, "{:?}", cal.primary_dir);
        assert!((cal.secondary_dir.x - q.x).abs() < 0.1 && (cal.secondary_dir.y - q.y).abs() < 0.1, "{:?}", cal.secondary_dir);
        for r in cal.rate {
            assert!((r - PX_PER_SIDEREAL).abs() < 1.5, "measured {r} px/s at 1x sidereal, expected {PX_PER_SIDEREAL}");
        }
    }

    #[test]
    fn seeing_jitter_alone_causes_no_pulses() {
        // no mount drift at all, +-3.5 px of seeing: a guider that chases the seeing would pulse constantly
        let mut w = World::new(Vec2::default(), 3.5);
        let cal = calibrate(&mut w);
        let mut g = Guider::new(w.pos, cal, GuideParams::default());
        let (pulses, worst) = run(&mut w, &mut g, 180.0, 180.0);
        assert_eq!(pulses, 0, "chased the seeing");
        assert!(worst < 2.0, "true position moved by {worst}");
    }

    #[test]
    fn corrects_realistic_mount_drift_with_few_gentle_pulses() {
        // ~8 px/min of drift (a typical residual after polar alignment) under +-3.5 px of seeing
        let mut w = World::new(Vec2::new(0.12, -0.08), 3.5);
        let cal = calibrate(&mut w);
        let mut g = Guider::new(w.pos, cal, GuideParams::default());
        run(&mut w, &mut g, 60.0, 0.0); // settle in
        let (pulses, worst) = run(&mut w, &mut g, 300.0, 300.0);
        // uncorrected, ~0.14 px/s would move the image ~42 px in 300 s
        assert!(worst < 8.0, "drift not held: worst true error {worst:.1} px");
        assert!(pulses <= 40, "too busy for this drift: {pulses} pulses in 300 s");
        assert!(pulses >= 4, "not correcting at all ({pulses})");
    }

    #[test]
    fn holds_even_a_harsh_drift_under_seeing() {
        // 28 px/min: far worse than a real mount; the guider must still hold it, just with more pulses
        let mut w = World::new(TRACKING_DRIFT, 3.5);
        let cal = calibrate(&mut w);
        let mut g = Guider::new(w.pos, cal, GuideParams::default());
        run(&mut w, &mut g, 60.0, 0.0);
        let (_, worst) = run(&mut w, &mut g, 180.0, 180.0);
        assert!(worst < 9.0, "harsh drift not held: worst true error {worst:.1} px");
    }

    #[test]
    fn large_offsets_are_pulled_back_reasonably_fast() {
        let mut w = World::new(TRACKING_DRIFT, 1.0);
        let cal = calibrate(&mut w);
        let target = w.pos;
        w.pos = w.pos + Vec2::new(40.0, -45.0); // e.g. after a manual slew
        let mut g = Guider::new(target, cal, GuideParams::default());
        let (_, worst) = run(&mut w, &mut g, 120.0, 20.0);
        assert!(worst < 10.0, "still {worst:.1} px off after 100 s");
    }

    #[test]
    fn parallel_axes_are_rejected() {
        assert!(Calibration::from_motion(Vec2::new(1.0, 0.0), Vec2::new(-1.0, 0.0), 4.0, 0.5).is_none());
    }

    #[test]
    fn calibration_fails_if_image_does_not_move() {
        let mut c = Calibrator::new();
        let t = Instant::now();
        let p = Vec2::new(10.0, 10.0);
        let _ = c.step(t, p);
        let _ = c.step(t + Duration::from_secs(5), p);
        assert!(matches!(c.step(t + Duration::from_secs(7), p), CalAction::Failed(_)));
    }
}
