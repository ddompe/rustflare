use rustflare_core::sim::SimSky;
use rustflare_core::{Axis, Mount, MountState, ParkState, RadPerSec, Result, SiteTime, SlewSpeed, TrackingRate, SIDEREAL_RATE};

pub struct SimMount {
    sky: SimSky,
    tracking: bool,
    guide: [f64; 2],
    slew: [f64; 2],
    /// A simulated GoTo keeps the mount "slewing" until this moment.
    goto_until: Option<std::time::Instant>,
}

impl SimMount {
    pub fn new(sky: SimSky) -> Self {
        SimMount { sky, tracking: false, guide: [0.0; 2], slew: [0.0; 2], goto_until: None }
    }
    fn push(&self) {
        self.sky.set_axis_rates(self.guide[0] + self.slew[0], self.guide[1] + self.slew[1]);
    }
}

impl Mount for SimMount {
    fn info(&self) -> String { "Simulated mount".into() }

    fn state(&mut self) -> Result<MountState> {
        Ok(MountState { tracking: Some(self.tracking), slewing: Some(self.goto_until.is_some_and(|t| std::time::Instant::now() < t)), at_home: Some(false), park: Some(ParkState::Unparked), alt_az: Some(false) })
    }

    fn set_site_time(&mut self, _st: &SiteTime) -> Result<()> { Ok(()) }

    fn goto_equatorial(&mut self, _ra: f64, _dec: f64) -> Result<()> {
        self.goto_until = Some(std::time::Instant::now() + std::time::Duration::from_secs(3));
        self.set_tracking(false)
    }

    fn go_home(&mut self) -> Result<()> {
        self.stop()?;
        self.set_tracking(false) // like the real mount, it does not track at home
    }

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
