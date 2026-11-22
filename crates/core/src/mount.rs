//! Mount abstraction. Types adapted from Vidoxide's `mount` module (MIT).

use crate::Result;
use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    /// RA / azimuth.
    Primary,
    /// Dec / altitude.
    Secondary,
}

pub const SECONDS_PER_DAY: f64 = 86164.09065;

/// Angular speed in radians per second.
#[derive(Copy, Clone, Debug, PartialEq, PartialOrd)]
pub struct RadPerSec(pub f64);

impl RadPerSec {
    pub fn is_zero(&self) -> bool { self.0 == 0.0 }
}

pub const SIDEREAL_RATE: RadPerSec = RadPerSec(2.0 * std::f64::consts::PI / SECONDS_PER_DAY);

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum SlewSpeed {
    Specific(RadPerSec),
    /// `true` = positive direction.
    Max(bool),
}

impl SlewSpeed {
    pub fn zero() -> SlewSpeed { SlewSpeed::Specific(RadPerSec(0.0)) }
    pub fn is_zero(&self) -> bool {
        matches!(self, SlewSpeed::Specific(s) if s.0 == 0.0)
    }
    pub fn positive(&self) -> bool {
        match self { SlewSpeed::Specific(s) => s.0 > 0.0, SlewSpeed::Max(p) => *p }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrackingRate {
    Sidereal,
    Solar,
    Lunar,
}

/// Park state as reported by the mount itself. Read-only information: RustFlare does not park or unpark (on ZWO
/// mounts that is a different thing from going home); it only knows that a parked mount refuses to move.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ParkState {
    Unparked,
    Parking,
    Parked,
}

/// What the mount itself says it is doing. Fields are `None` when the driver cannot tell. The mount can be
/// changed from outside (its own buttons, another app), so this is read back, never assumed.
#[derive(Copy, Clone, Debug, Default, PartialEq, Serialize)]
pub struct MountState {
    pub tracking: Option<bool>,
    pub slewing: Option<bool>,
    pub at_home: Option<bool>,
    pub park: Option<ParkState>,
    /// Alt-azimuth mode (field rotation; park is not available).
    pub alt_az: Option<bool>,
}

/// How the mount is connected and whether that link is still alive (a Bluetooth link can drop at any time).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LinkInfo {
    /// `"usb"` or `"ble"`.
    pub kind: &'static str,
    pub up: bool,
}

/// Observer site and clock, as the mount needs them to turn sky coordinates into a pointing.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SiteTime {
    pub latitude_deg: f64,
    /// East positive (a site west of Greenwich is negative).
    pub longitude_east_deg: f64,
    pub unix_secs: i64,
    /// Local time minus UTC, minutes (UTC-6 is -360).
    pub utc_offset_minutes: i32,
}

pub trait Mount: Send {
    /// Tells the mount where and when it is (the official app does this on every connect).
    fn set_site_time(&mut self, _st: &SiteTime) -> Result<()> { Err(crate::Error::Unsupported("set site/time".into())) }
    /// Starts a GoTo slew to equatorial coordinates of date (RA in hours, Dec in degrees). Moves the mount.
    fn goto_equatorial(&mut self, _ra_hours: f64, _dec_deg: f64) -> Result<()> { Err(crate::Error::Unsupported("goto".into())) }
    /// The transport and its health; `None` for drivers without a physical link (the simulator).
    fn link(&self) -> Option<LinkInfo> { None }
    /// Asks the mount for its current state (read-only).
    fn state(&mut self) -> Result<MountState> { Ok(MountState::default()) }
    /// Where the mount says it is pointing: (RA hours, Dec degrees). `None` if the driver cannot tell. Read-only.
    fn equatorial(&mut self) -> Result<Option<(f64, f64)>> { Ok(None) }
    /// Moves to the home (zero) position. Moves the mount.
    fn go_home(&mut self) -> Result<()> { Err(crate::Error::Unsupported("go home".into())) }

    fn info(&self) -> String;
    fn set_tracking(&mut self, enabled: bool) -> Result<()>;
    /// Selects the sky tracking rate (the Sun needs `Solar`).
    fn set_tracking_rate(&mut self, rate: TrackingRate) -> Result<()>;
    /// Sets guiding speeds on both axes (signed; zero stops). Requires tracking.
    fn guide(&mut self, axis1: RadPerSec, axis2: RadPerSec) -> Result<()>;
    /// Zero speed stops slewing (tracking is unaffected).
    fn slew(&mut self, axis: Axis, speed: SlewSpeed) -> Result<()>;
    fn slewing_speed_supported(&self, speed: RadPerSec) -> bool;
    /// Stops all motion. Does *not* change tracking.
    fn stop(&mut self) -> Result<()>;
}
