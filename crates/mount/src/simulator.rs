use rustflare_core::sim::SimSky;
use rustflare_core::{Axis, Mount, RadPerSec, Result, SlewSpeed, TrackingRate, SIDEREAL_RATE};

pub struct SimMount {
    sky: SimSky,
    tracking: bool,
    guide: [f64; 2],
    slew: [f64; 2],
}

impl SimMount {
    pub fn new(sky: SimSky) -> Self {
        SimMount { sky, tracking: false, guide: [0.0; 2], slew: [0.0; 2] }
    }
    fn push(&self) {
        self.sky.set_axis_rates(self.guide[0] + self.slew[0], self.guide[1] + self.slew[1]);
    }
}

impl Mount for SimMount {
    fn info(&self) -> String { "Simulated mount".into() }

    fn set_tracking(&mut self, enabled: bool) -> Result<()> {
        self.tracking = enabled;
        self.sky.set_tracking(enabled);
        Ok(())
    }

    fn set_tracking_rate(&mut self, _rate: TrackingRate) -> Result<()> { Ok(()) }

    fn guide(&mut self, a1: RadPerSec, a2: RadPerSec) -> Result<()> {
        if !(a1.is_zero() && a2.is_zero()) && !self.tracking {
            return Err(rustflare_core::Error::Mount("cannot guide when tracking is disabled".into()));
        }
        self.guide = [a1.0 / SIDEREAL_RATE.0, a2.0 / SIDEREAL_RATE.0];
        self.push();
        Ok(())
    }

    fn slew(&mut self, axis: Axis, speed: SlewSpeed) -> Result<()> {
        let mult = match speed {
            SlewSpeed::Specific(s) => s.0 / SIDEREAL_RATE.0,
            SlewSpeed::Max(p) => if p { 1440.0 } else { -1440.0 },
        };
        match axis { Axis::Primary => self.slew[0] = mult, Axis::Secondary => self.slew[1] = mult }
        self.push();
        Ok(())
    }

    fn slewing_speed_supported(&self, _speed: RadPerSec) -> bool { true }

    fn stop(&mut self) -> Result<()> {
        self.guide = [0.0; 2];
        self.slew = [0.0; 2];
        self.push();
        Ok(())
    }
}
