//! Calibration and guiding. The image-to-axes mapping and direction maths are
//! ported from Vidoxide's `guiding.rs`; the timing logic is restructured as
//! pure state machines driven by the caller.

use serde::Serialize;
use std::time::{Duration, Instant};
use rustflare_core::{Mat2, Vec2};

#[derive(Copy, Clone, Debug, PartialEq, Serialize)]
pub struct Calibration {
    /// Image-space unit vector of positive primary-axis motion.
    pub primary_dir: Vec2,
    /// Image-space unit vector of positive secondary-axis motion.
    pub secondary_dir: Vec2,
    pub img_to_axes: Mat2,
}

impl Calibration {
    /// Fails if the directions are (nearly) parallel.
    pub fn from_dirs(primary_dir: Vec2, secondary_dir: Vec2) -> Option<Calibration> {
        let p = primary_dir.normalized()?;
        let s = secondary_dir.normalized()?;
        if (p.x * s.y - p.y * s.x).abs() < 0.2 {
            return None;
        }
        Some(Calibration { primary_dir: p, secondary_dir: s, img_to_axes: Mat2::from_cols(p, s).invert()? })
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

#[derive(Copy, Clone, Debug)]
pub struct GuideParams {
    /// Acceptable error in pixels.
    pub margin_px: f64,
    /// Guide speed as a multiple of sidereal.
    pub speed: f64,
    pub check_interval: Duration,
    pub correct_interval: Duration,
}

impl Default for GuideParams {
    fn default() -> Self {
        GuideParams { margin_px: 5.0, speed: 0.5, check_interval: Duration::from_millis(2000), correct_interval: Duration::from_millis(1000) }
    }
}

/// Keeps the tracked position on `target` by issuing guide pulses.
pub struct Guider {
    pub target: Vec2,
    cal: Calibration,
    params: GuideParams,
    correcting: bool,
    next_at: Option<Instant>,
}

impl Guider {
    pub fn new(target: Vec2, cal: Calibration, params: GuideParams) -> Guider {
        Guider { target, cal, params, correcting: false, next_at: None }
    }

    /// Call on every new measurement; returns a command only when the mount needs one.
    pub fn step(&mut self, now: Instant, pos: Vec2) -> Option<GuideCommand> {
        if let Some(t) = self.next_at {
            if now < t { return None; }
        }
        let err = self.target - pos;
        if err.x.abs() > self.params.margin_px || err.y.abs() > self.params.margin_px {
            let dir = self.cal.guiding_direction(err)?;
            self.correcting = true;
            self.next_at = Some(now + self.params.correct_interval);
            Some(GuideCommand { primary: dir.x * self.params.speed, secondary: dir.y * self.params.speed })
        } else {
            self.next_at = Some(now + self.params.check_interval);
            if self.correcting {
                self.correcting = false;
                Some(GuideCommand::STOP)
            } else {
                None
            }
        }
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
    dirs: [Option<Vec2>; 2],
    pub move_time: Duration,
    pub settle_time: Duration,
    pub speed: f64,
    pub min_move_px: f64,
}

impl Calibrator {
    pub fn new() -> Calibrator {
        Calibrator {
            phase: Phase::Init,
            dirs: [None; 2],
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
                self.dirs[axis] = d.normalized();
                if axis == 0 {
                    self.phase = Phase::Moving { axis: 1, since: now, from: pos };
                    CalAction::Guide(self.cmd(1))
                } else {
                    match Calibration::from_dirs(self.dirs[0].unwrap(), self.dirs[1].unwrap()) {
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

    /// Fake world: image position integrates drift + commanded guide motion in 100 ms steps.
    struct World { t0: Instant, now: Instant, pos: Vec2, cmd: GuideCommand }
    impl World {
        fn new() -> World { let t = Instant::now(); World { t0: t, now: t, pos: Vec2::new(640.0, 480.0), cmd: GuideCommand::STOP } }
        fn tick(&mut self) {
            let dt = 0.1;
            let (p, q) = axis_dirs();
            self.pos = self.pos + (TRACKING_DRIFT + p * (self.cmd.primary * PX_PER_SIDEREAL) + q * (self.cmd.secondary * PX_PER_SIDEREAL)) * dt;
            self.now += Duration::from_millis(100);
        }
        fn elapsed(&self) -> f64 { self.now.duration_since(self.t0).as_secs_f64() }
    }

    fn calibrate(w: &mut World) -> Calibration {
        let mut c = Calibrator::new();
        for _ in 0..1000 {
            match c.step(w.now, w.pos) {
                CalAction::Guide(g) => w.cmd = g,
                CalAction::Done(cal) => { w.cmd = GuideCommand::STOP; return cal; }
                CalAction::Failed(e) => panic!("{e}"),
                CalAction::None => {}
            }
            w.tick();
        }
        panic!("calibration never finished");
    }

    #[test]
    fn calibration_discovers_axis_directions() {
        let mut w = World::new();
        let cal = calibrate(&mut w);
        let (p, q) = axis_dirs();
        // drift (0.4,-0.25 px/s) perturbs the measurement a little, hence the tolerance
        assert!((cal.primary_dir.x - p.x).abs() < 0.1 && (cal.primary_dir.y - p.y).abs() < 0.1, "{:?}", cal.primary_dir);
        assert!((cal.secondary_dir.x - q.x).abs() < 0.1 && (cal.secondary_dir.y - q.y).abs() < 0.1, "{:?}", cal.secondary_dir);
    }

    #[test]
    fn guider_pulls_position_back_to_target() {
        let mut w = World::new();
        let cal = calibrate(&mut w);
        // knock the Sun 60 px off target and make drift worse
        w.pos = w.pos + Vec2::new(40.0, -45.0);
        let target = Vec2::new(640.0, 480.0);
        let mut g = Guider::new(target, cal, GuideParams::default());
        let mut worst_late: f64 = 0.0;
        let start = w.elapsed();
        while w.elapsed() - start < 60.0 {
            if let Some(c) = g.step(w.now, w.pos) { w.cmd = c; }
            w.tick();
            if w.elapsed() - start > 30.0 {
                worst_late = worst_late.max((w.pos - target).len());
            }
        }
        assert!(worst_late < 12.0, "still {worst_late:.1}px off after 30s");
    }

    #[test]
    fn parallel_axes_are_rejected() {
        assert!(Calibration::from_dirs(Vec2::new(1.0, 0.0), Vec2::new(-1.0, 0.0)).is_none());
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
