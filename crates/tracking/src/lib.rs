//! Solar image tracking, mount calibration and guiding.
//!
//! Tracker algorithms (anchor block matching) and the guiding maths are ported
//! from Vidoxide (MIT). Everything here is pure logic with injected time, so it
//! is unit-testable without hardware (ADR 0008).

pub mod guiding;
pub mod solar;
pub mod tracker;

pub use guiding::{CalAction, Calibration, Calibrator, GuideCommand, GuideParams, Guider};
pub use tracker::{fit_limb_auto, limb_fits, locate_sun, LimbFit, Position, SunLocation, TrackError, Tracker};
