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

/// The longest single guide command: the mount ends a timed pulse by itself after 3 s.
const MAX_PULSE_CHUNK: Duration = Duration::from_millis(2500);

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
    /// Total image motion (pixels) the pulses sent so far were meant to produce. Position minus this is what the mount
    /// would have done on its own: its drift, whatever the guider did.
    applied: Vec2,
    /// Measurements arrive in short bursts with long gaps (lane recordings): samples are only collected by `step`, and
    /// the caller says when to judge them (`judge_now`), e.g. when a burst ends.
    sparse: bool,
    /// The mount's learned unguided drift, px/s (from `DriftModel`): corrections aim where the Sun will be half-way to the
    /// next judgement, so a constant drift does not leave a constant offset (drift x interval / gain) behind.
    drift: Option<Vec2>,
    last_judge: Option<Instant>,
}

impl Guider {
    pub fn new(target: Vec2, cal: Calibration, params: GuideParams) -> Guider {
        Guider { target, cal, params, samples: VecDeque::new(), pending: VecDeque::new(), pulse_end: None, judge_after: None, filtered_error: None, quick: false, applied: Vec2::default(), sparse: false, drift: None, last_judge: None }
    }

    /// Image motion the guide pulses so far were meant to cause, in pixels (from the calibration).
    pub fn applied(&self) -> Vec2 {
        self.applied
    }

    /// Forget the samples gathered so far (they came from another source, e.g. dead reckoning).
    pub fn reset_samples(&mut self) {
        self.samples.clear();
        self.filtered_error = None;
    }

    /// Offset of the *averaged* position from the target (what the guider acts on), once available.
    pub fn filtered_error(&self) -> Option<Vec2> {
        self.filtered_error
    }

    pub fn set_sparse(&mut self, sparse: bool) {
        self.sparse = sparse;
    }

    /// The mount's unguided drift in image pixels per second, once known (None: no feed-forward).
    pub fn set_drift(&mut self, drift: Option<Vec2>) {
        self.drift = drift;
    }

    /// Time keeping only: ends or continues a running pulse. Call on frames that carry no measurement.
    pub fn tick(&mut self, now: Instant) -> Option<GuideCommand> {
        let end = self.pulse_end?;
        if now < end {
            return None;
        }
        match self.pending.pop_front() {
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
        }
    }

    /// Call on every new measurement; returns a command only when the mount needs one.
    pub fn step(&mut self, now: Instant, pos: Vec2) -> Option<GuideCommand> {
        // 1. a pulse is running (or just finished)
        if self.pulse_end.is_some() {
            return self.tick(now);
        }
        // 2. gather the averaging window
        self.samples.push_back((now, pos));
        let window = if self.sparse { Duration::from_secs(20) } else { self.params.window };
        while self.samples.front().is_some_and(|(t, _)| now.duration_since(*t) > window) {
            self.samples.pop_front();
        }
        if self.sparse {
            return None; // the caller judges when a burst ends
        }
        if self.judge_after.is_some_and(|t| now < t) {
            return None;
        }
        let span = now.duration_since(self.samples.front()?.0);
        let needed = if self.quick { Duration::from_millis(1500) } else { self.params.window / 2 };
        if span < needed {
            return None; // not enough data to tell drift from seeing
        }
        self.judge(now)
    }

    /// Judges the samples collected so far (at least a handful) and starts a correction if one is needed. For sparse
    /// measurements, called when a burst of them ends; the pulses then run while the next frames are not measured.
    pub fn judge_now(&mut self, now: Instant) -> Option<GuideCommand> {
        if self.pulse_end.is_some() || self.samples.len() < 8 || self.judge_after.is_some_and(|t| now < t) {
            return None;
        }
        self.judge(now)
    }

    fn judge(&mut self, now: Instant) -> Option<GuideCommand> {
        let n = self.samples.len() as f64;
        let mean = self.samples.iter().fold(Vec2::default(), |a, (_, p)| a + *p) * (1.0 / n);
        let offset = self.target - mean;
        self.filtered_error = Some(offset);
        // feed-forward: where the Sun will be half-way to the next judgement, if nothing is done
        let interval = self.last_judge.map_or(self.params.min_interval, |t| now.duration_since(t)).clamp(self.params.min_interval, Duration::from_secs(120));
        self.last_judge = Some(now);
        let err = match self.drift {
            Some(d) => offset - d * (interval.as_secs_f64() * 0.5),
            None => offset,
        };
        if err.len() < self.params.deadband_px {
            self.quick = false;
            if self.sparse { self.samples.clear(); } // the next burst starts a fresh average
            return None;
        }
        // 3. plan gentle pulses, one axis at a time: image motion wanted = gain x error, in axis coordinates
        let big = err.len() > self.params.fast_above_px;
        self.quick = big;
        let (speed, mut max_pulse) = if big { (self.params.fast_speed, self.params.max_pulse * 2) } else { (self.params.speed, self.params.max_pulse) };
        if self.sparse {
            // judged rarely (once per burst): the correction for the whole gap must fit in one go, so allow it to run as long as the gap
            max_pulse = max_pulse.max(interval.mul_f64(0.6));
        }
        let a = self.cal.img_to_axes.apply(err * self.params.gain);
        for (i, px) in [a.x, a.y].into_iter().enumerate() {
            let rate = self.cal.rate[i] * speed; // image px per second while pulsing
            if rate <= 0.0 {
                continue;
            }
            let dur = Duration::from_secs_f64((px.abs() / rate).min(max_pulse.as_secs_f64()));
            if dur < Duration::from_millis(100) {
                continue; // below what the measurement loop can time
            }
            let v = if px > 0.0 { speed } else { -speed };
            let cmd = if i == 0 { GuideCommand { primary: v, secondary: 0.0 } } else { GuideCommand { primary: 0.0, secondary: v } };
            // The mount stops a guide pulse by itself after 3 s: a longer correction is sent as back-to-back pulses of at most 2.5 s.
            let mut left = dur;
            while left > Duration::ZERO {
                let chunk = left.min(MAX_PULSE_CHUNK);
                self.pending.push_back((cmd, chunk));
                left -= chunk;
            }
            let dir = if i == 0 { self.cal.primary_dir } else { self.cal.secondary_dir };
            self.applied = self.applied + dir * (self.cal.rate[i] * v * dur.as_secs_f64());
        }
        let (cmd, dur) = self.pending.pop_front()?;
        self.pulse_end = Some(now + dur);
        self.samples.clear();
        Some(cmd)
    }
}

/// What the mount does on its own, learned while guiding works: position minus the motion the pulses caused is the drift,
/// whatever the guider did about it. Its least-squares slope over the last minutes is the unguided drift in pixels per
/// second, along both image axes at once (so per mount axis too: the calibration maps one onto the other).
#[derive(Default)]
pub struct DriftModel {
    samples: VecDeque<(Instant, Vec2)>,
}

impl DriftModel {
    const SPAN: Duration = Duration::from_secs(600);
    const MIN_SPAN: Duration = Duration::from_secs(60);

    pub fn clear(&mut self) {
        self.samples.clear();
    }

    /// A good measurement while guiding: `pos` is where the Sun was seen, `applied` the guider's `applied()` at that moment.
    /// Keeps about one sample a second.
    pub fn observe(&mut self, now: Instant, pos: Vec2, applied: Vec2) {
        if self.samples.back().is_some_and(|(t, _)| now.duration_since(*t) < Duration::from_secs(1)) {
            return;
        }
        self.samples.push_back((now, pos - applied));
        while self.samples.front().is_some_and(|(t, _)| now.duration_since(*t) > Self::SPAN) {
            self.samples.pop_front();
        }
    }

    /// Unguided drift, px/s, once there are at least a minute of good measurements.
    pub fn drift(&self) -> Option<Vec2> {
        let (first, last) = (self.samples.front()?, self.samples.back()?);
        if self.samples.len() < 20 || last.0.duration_since(first.0) < Self::MIN_SPAN {
            return None;
        }
        let n = self.samples.len() as f64;
        let ts: Vec<f64> = self.samples.iter().map(|(t, _)| t.duration_since(first.0).as_secs_f64()).collect();
        let tm = ts.iter().sum::<f64>() / n;
        let (mx, my) = (self.samples.iter().map(|(_, p)| p.x).sum::<f64>() / n, self.samples.iter().map(|(_, p)| p.y).sum::<f64>() / n);
        let stt: f64 = ts.iter().map(|t| (t - tm) * (t - tm)).sum();
        if stt < 1e-9 {
            return None;
        }
        let (mut sx, mut sy) = (0.0, 0.0);
        for ((_, p), t) in self.samples.iter().zip(&ts) {
            sx += (t - tm) * (p.x - mx);
            sy += (t - tm) * (p.y - my);
        }
        Some(Vec2::new(sx / stt, sy / stt))
    }
}

/// Stands in for the measurement while the Sun cannot be seen (cloud, trees, an eclipse): the last good position, moved on
/// by the learned drift and by whatever the guide pulses did since, so the guider keeps the framing instead of giving up.
pub struct DeadReckoning {
    t0: Instant,
    pos0: Vec2,
    applied0: Vec2,
    drift: Vec2,
}

impl DeadReckoning {
    pub fn new(t0: Instant, pos0: Vec2, applied0: Vec2, drift: Vec2) -> DeadReckoning {
        DeadReckoning { t0, pos0, applied0, drift }
    }

    /// Where the Sun should be now, given the guider's current `applied()`.
    pub fn position(&self, now: Instant, applied: Vec2) -> Vec2 {
        self.pos0 + self.drift * now.duration_since(self.t0).as_secs_f64() + (applied - self.applied0)
    }

    /// How far the pulses have moved the image since the last real measurement, pixels.
    pub fn moved_px(&self, applied: Vec2) -> f64 {
        (applied - self.applied0).len()
    }

    pub fn age(&self, now: Instant) -> Duration {
        now.duration_since(self.t0)
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
    Moving { axis: usize, since: Instant, from: Vec2, last_cmd: Instant },
    Settling { axis: usize, since: Instant, from: Vec2 },
}

/// Moves each axis in turn and measures the image displacement to learn the axis directions.
pub struct Calibrator {
    phase: Phase,
    /// Recent positions: the start and end of each move use their median, so one bad frame (shadow, cloud) cannot spoil it.
    recent: VecDeque<(Instant, Vec2)>,
    init_since: Option<Instant>,
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
            recent: VecDeque::new(),
            init_since: None,
            motion: [None; 2],
            move_time: Duration::from_secs(4),
            settle_time: Duration::from_millis(1500),
            speed: 0.5,
            min_move_px: 5.0,
        }
    }

    fn cmd(&self, axis: usize) -> GuideCommand {
        if axis == 0 { GuideCommand { primary: self.speed, secondary: 0.0 } } else { GuideCommand { primary: 0.0, secondary: self.speed } }
    }

    /// Coordinate-wise median of the positions of the last `window`; `None` with fewer than 3 samples.
    fn median(&self, now: Instant, window: Duration) -> Option<Vec2> {
        let mut xs: Vec<f64> = vec![];
        let mut ys: Vec<f64> = vec![];
        for (t, p) in &self.recent {
            if now.duration_since(*t) <= window {
                xs.push(p.x);
                ys.push(p.y);
            }
        }
        if xs.len() < 3 {
            return None;
        }
        xs.sort_by(|a, b| a.total_cmp(b));
        ys.sort_by(|a, b| a.total_cmp(b));
        Some(Vec2::new(xs[xs.len() / 2], ys[ys.len() / 2]))
    }

    pub fn step(&mut self, now: Instant, pos: Vec2) -> CalAction {
        self.recent.push_back((now, pos));
        while self.recent.front().is_some_and(|(t, _)| now.duration_since(*t) > Duration::from_secs(3)) {
            self.recent.pop_front();
        }
        match self.phase {
            Phase::Init => {
                // look at the Sun for a moment first: the start of the first move is the median of that
                let since = *self.init_since.get_or_insert(now);
                if now.duration_since(since) < Duration::from_millis(1500) {
                    return CalAction::None;
                }
                let from = self.median(now, Duration::from_millis(1500)).unwrap_or(pos);
                self.phase = Phase::Moving { axis: 0, since: now, from, last_cmd: now };
                CalAction::Guide(self.cmd(0))
            }
            Phase::Moving { axis, since, from, last_cmd } => {
                if now.duration_since(since) >= self.move_time {
                    self.phase = Phase::Settling { axis, since: now, from };
                    CalAction::Guide(GuideCommand::STOP)
                } else if now.duration_since(last_cmd) >= MAX_PULSE_CHUNK {
                    // the mount ends a guide command by itself after 3 s: keep the move going, or the motion measured is shorter than assumed
                    self.phase = Phase::Moving { axis, since, from, last_cmd: now };
                    CalAction::Guide(self.cmd(axis))
                } else {
                    CalAction::None
                }
            }
            Phase::Settling { axis, since, from } => {
                if now.duration_since(since) < self.settle_time {
                    return CalAction::None;
                }
                let end = self.median(now, self.settle_time).unwrap_or(pos);
                let d = end - from;
                if d.len() < self.min_move_px {
                    return CalAction::Failed(format!(
                        "image moved only {:.1}px on axis {} (is sky tracking on and the guide rate adequate?)",
                        d.len(), axis + 1
                    ));
                }
                self.motion[axis] = Some(d);
                if axis == 0 {
                    self.phase = Phase::Moving { axis: 1, since: now, from: end, last_cmd: now };
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
    struct World { t0: Instant, now: Instant, pos: Vec2, cmd: GuideCommand, cmd_age: f64, drift: Vec2, jitter: f64, rng: u64 }
    impl World {
        fn new(drift: Vec2, jitter: f64) -> World {
            let t = Instant::now();
            World { t0: t, now: t, pos: Vec2::new(640.0, 480.0), cmd: GuideCommand::STOP, cmd_age: 0.0, drift, jitter, rng: 0x2545F4914F6CDD1D }
        }
        fn noise(&mut self) -> f64 {
            self.rng ^= self.rng << 13; self.rng ^= self.rng >> 7; self.rng ^= self.rng << 17;
            ((self.rng >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0) * self.jitter
        }
        /// Sends a command to the mount: it restarts the mount's own 3 s timer.
        fn issue(&mut self, c: GuideCommand) { self.cmd = c; self.cmd_age = 0.0; }
        fn tick(&mut self) {
            let (p, q) = axis_dirs();
            // like the real mount: a guide command runs for at most 3 s on its own
            self.cmd_age += 0.1;
            let c = if self.cmd_age > 3.0 { GuideCommand::STOP } else { self.cmd };
            self.pos = self.pos + (self.drift + p * (c.primary * PX_PER_SIDEREAL) + q * (c.secondary * PX_PER_SIDEREAL)) * 0.1;
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
                CalAction::Guide(g) => w.issue(g),
                CalAction::Done(cal) => { w.issue(GuideCommand::STOP); return cal; }
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
                w.issue(c);
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
    fn dead_reckoning_keeps_the_framing_while_the_sun_is_hidden() {
        let mut w = World::new(TRACKING_DRIFT, 2.0);
        let cal = calibrate(&mut w);
        let target = w.pos;
        let mut g = Guider::new(target, cal, GuideParams::default());
        let mut dm = DriftModel::default();
        // five minutes of good guiding teaches the drift
        let start = w.elapsed();
        while w.elapsed() - start < 300.0 {
            let m = w.measured();
            dm.observe(w.now, m, g.applied());
            if let Some(c) = g.step(w.now, m) { w.issue(c); }
            w.tick();
        }
        let d = dm.drift().expect("drift learned");
        assert!((d - TRACKING_DRIFT).len() < 0.15, "learned {d:?} but the mount drifts {TRACKING_DRIFT:?}");
        // then four minutes in which nothing is measured: with and without dead reckoning
        let (mut with, mut without) = (World::new(TRACKING_DRIFT, 2.0), World::new(TRACKING_DRIFT, 2.0));
        let _ = (&mut with, &mut without);
        let dr = DeadReckoning::new(w.now, w.pos, g.applied(), d);
        let start = w.elapsed();
        let mut worst = 0f64;
        while w.elapsed() - start < 240.0 {
            let virt = dr.position(w.now, g.applied());
            if let Some(c) = g.step(w.now, virt) { w.issue(c); }
            w.tick();
            worst = worst.max((w.pos - target).len());
        }
        assert!(worst < 12.0, "the framing moved {worst:.1} px in four minutes of dead reckoning");
        // for comparison, doing nothing for the same time moves it by drift x time
        assert!(TRACKING_DRIFT.len() * 240.0 > 100.0);
    }

    /// Lane recordings: a one-second burst of measurements every 12 s, judged when the burst ends. Feed-forward from the
    /// learned drift removes the offset a purely proportional correction leaves behind against a steady drift.
    #[test]
    fn sparse_guiding_with_feed_forward_holds_the_framing() {
        fn run_bursts(ff: bool) -> f64 {
            let mut w = World::new(TRACKING_DRIFT, 1.0);
            let cal = calibrate(&mut w);
            let target = w.pos;
            let mut g = Guider::new(target, cal, GuideParams::default());
            g.set_sparse(true);
            if ff { g.set_drift(Some(TRACKING_DRIFT)); }
            let mut worst = 0f64;
            for i in 0..6000u32 {
                // 0.1 s per tick: a burst of 1 s of measurements every 12 s, judged on the tick after it
                let phase = i % 120;
                if phase < 10 {
                    let m = w.measured();
                    if let Some(c) = g.step(w.now, m) { w.issue(c); }
                } else {
                    if phase == 10 {
                        let r = g.judge_now(w.now);
                        if let Some(c) = r { w.issue(c); }
                    }
                    if let Some(c) = g.tick(w.now) { w.issue(c); }
                }
                w.tick();
                if i > 3000 { worst = worst.max((w.pos - target).len()); }
            }
            worst
        }
        let (plain, ff) = (run_bursts(false), run_bursts(true));
        assert!(ff < 6.0, "with feed-forward the framing still moved {ff:.1} px");
        assert!(ff < plain, "feed-forward ({ff:.1} px) should beat the plain proportional correction ({plain:.1} px)");
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
        let mut failed = false;
        for i in 0..200 {
            if matches!(c.step(t + Duration::from_millis(100 * i), p), CalAction::Failed(_)) { failed = true; break; }
        }
        assert!(failed, "a calibration where the image never moves must fail");
    }

    #[test]
    fn calibration_ignores_a_few_bad_frames() {
        // the same world as the other calibration test, with every tenth measurement a wild outlier (shadow, cloud)
        let mut w = World::new(TRACKING_DRIFT, 1.0);
        let mut c = Calibrator::new();
        let mut n = 0u32;
        for _ in 0..2000 {
            n += 1;
            let mut m = w.measured();
            if n % 10 == 0 { m = m + Vec2::new(300.0, -250.0); }
            match c.step(w.now, m) {
                CalAction::Guide(g) => w.issue(g),
                CalAction::Done(cal) => {
                    let (p, q) = axis_dirs();
                    assert!((cal.primary_dir - p).len() < 0.15 && (cal.secondary_dir - q).len() < 0.15, "{cal:?}");
                    return;
                }
                CalAction::Failed(e) => panic!("{e}"),
                CalAction::None => {}
            }
            w.tick();
        }
        panic!("calibration never finished");
    }
}
