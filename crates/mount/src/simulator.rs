use rustflare_core::sim::SimSky;
use rustflare_core::{Axis, Mount, MountSiteTime, MountState, ParkState, RadPerSec, Result, SiteTime, SlewSpeed, TrackingRate, SIDEREAL_RATE};

/// Test hook: what the simulated mount should hold as its site and clock, applied by the next simulated mount that reads or is written to.
/// (The engine owns the mount behind a trait object, so the hook is a process-wide value; only the simulator reads it.)
static PENDING_CLOCK: std::sync::Mutex<Option<SimClock>> = std::sync::Mutex::new(None);

/// A simulated mount's own site and clock: `time_offset_s` is the mount's clock minus the real one.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct SimClock {
    pub time_offset_s: i64,
    pub latitude_deg: Option<f64>,
    pub longitude_east_deg: Option<f64>,
}

/// Makes the simulated mount hold `clock` from now on (so the Aim tab and the tests can exercise "the mount's time or place differs").
pub fn set_sim_clock(clock: SimClock) {
    *PENDING_CLOCK.lock().unwrap_or_else(|e| e.into_inner()) = Some(clock);
}

pub struct SimMount {
    clock: SimClock,
    sky: SimSky,
    tracking: bool,
    guide: [f64; 2],
    slew: [f64; 2],
    /// A simulated GoTo keeps the mount "slewing" until this moment.
    goto_until: Option<std::time::Instant>,
    /// Where the simulated mount believes it points (RA hours, Dec degrees).
    pointing: (f64, f64),
    /// At the home position (after `go_home`, until it moves again).
    at_home: bool,
}

impl SimMount {
    pub fn new(sky: SimSky) -> Self {
        SimMount { clock: SimClock::default(), sky, tracking: false, guide: [0.0; 2], slew: [0.0; 2], goto_until: None, pointing: (15.8, -19.9), at_home: false }
    }
    fn take_hook(&mut self) {
        if let Some(c) = PENDING_CLOCK.lock().unwrap_or_else(|e| e.into_inner()).take() {
            self.clock = c;
        }
    }
    fn push(&self) {
        self.sky.set_axis_rates(self.guide[0] + self.slew[0], self.guide[1] + self.slew[1]);
    }
}

impl Mount for SimMount {
    fn info(&self) -> String { "Simulated mount".into() }

    fn state(&mut self) -> Result<MountState> {
        Ok(MountState { tracking: Some(self.tracking), slewing: Some(self.goto_until.is_some_and(|t| std::time::Instant::now() < t)), at_home: Some(self.at_home && !self.goto_until.is_some_and(|t| std::time::Instant::now() < t)), park: Some(ParkState::Unparked), alt_az: Some(false) })
    }

    fn set_site_time(&mut self, st: &SiteTime) -> Result<()> {
        self.take_hook();
        self.clock = SimClock { time_offset_s: st.unix_secs - now_unix(), latitude_deg: Some(st.latitude_deg), longitude_east_deg: Some(st.longitude_east_deg) };
        Ok(())
    }

    fn read_site_time(&mut self) -> Result<Option<MountSiteTime>> {
        self.take_hook();
        Ok(Some(MountSiteTime { latitude_deg: self.clock.latitude_deg, longitude_east_deg: self.clock.longitude_east_deg, unix_secs: Some(now_unix() + self.clock.time_offset_s), utc_offset_minutes: Some(0) }))
    }

    fn equatorial(&mut self) -> Result<Option<(f64, f64)>> { Ok(Some(self.pointing)) }

    fn goto_equatorial(&mut self, ra: f64, dec: f64) -> Result<()> {
        self.pointing = (ra, dec);
        self.at_home = false;
        self.goto_until = Some(std::time::Instant::now() + std::time::Duration::from_secs(3));
        self.set_tracking(false)
    }

    fn go_home(&mut self) -> Result<()> {
        self.stop()?;
        self.goto_until = Some(std::time::Instant::now() + std::time::Duration::from_secs(2)); // it takes a moment to get there
        self.at_home = true;
        self.set_tracking(false) // like the real mount, it does not track at home
    }

    fn set_tracking(&mut self, enabled: bool) -> Result<()> {
        if enabled { self.at_home = false; }
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

fn now_unix() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hook_sets_what_the_simulated_mount_reports_and_a_sync_corrects_it() {
        let mut m = SimMount::new(SimSky::default());
        set_sim_clock(SimClock { time_offset_s: 180, latitude_deg: Some(10.0), longitude_east_deg: Some(-80.0) });
        let r = m.read_site_time().unwrap().unwrap();
        assert!((r.unix_secs.unwrap() - now_unix() - 180).abs() <= 1);
        assert_eq!((r.latitude_deg, r.longitude_east_deg), (Some(10.0), Some(-80.0)));
        m.set_site_time(&SiteTime { latitude_deg: 40.0, longitude_east_deg: -105.0, unix_secs: now_unix(), utc_offset_minutes: 0 }).unwrap();
        let r = m.read_site_time().unwrap().unwrap();
        assert!((r.unix_secs.unwrap() - now_unix()).abs() <= 1);
        assert_eq!((r.latitude_deg, r.longitude_east_deg), (Some(40.0), Some(-105.0)));
    }
}
