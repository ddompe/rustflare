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

pub trait Mount: Send {
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
