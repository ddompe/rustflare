//! Switching a cooled sensor off: ramp, do not jump.
//!
//! A sensor held 30 degrees or more below ambient that loses its cooler at once warms by several degrees a second. That thermal shock
//! is hard on the sensor window and the seal, and dew forms on a cold window. So a request to switch the cooler off, while the sensor is
//! cold, raises the target a degree at a time and only switches off when the sensor is near ambient (or the cooler has nothing left to do).
//! The ambient temperature is not known, so "near ambient" is a fixed level, [`WARM_ENOUGH_C`].

use std::time::Duration;

/// At or above this sensor temperature the cooler is simply switched off.
pub const WARM_ENOUGH_C: f64 = 15.0;
/// One degree every this long, about 4 degrees a minute.
pub const STEP_EVERY: Duration = Duration::from_secs(15);
const STEP_C: f64 = 1.0;

#[derive(Debug, PartialEq)]
pub enum Warmup {
    /// Switch the cooler off now.
    Finish,
    /// Move the target to this value.
    Raise(f64),
    /// Nothing yet: the sensor has not caught up with the target.
    Wait,
}

/// The next move of a warm-up, given the sensor temperature, the target, the cooler power and the highest target the camera accepts.
pub fn warmup_step(temp_c: f64, target_c: f64, power_pct: Option<f64>, max_c: f64) -> Warmup {
    if temp_c >= WARM_ENOUGH_C {
        return Warmup::Finish;
    }
    // idle: the target is at or above the sensor and the cooler does no work
    if target_c >= temp_c && power_pct.is_some_and(|p| p <= 1.0) {
        return Warmup::Finish;
    }
    // do not run ahead of the sensor, or the target is up at the maximum while the sensor is still cold
    if target_c < max_c && target_c <= temp_c + 2.0 {
        return Warmup::Raise((target_c + STEP_C).min(max_c));
    }
    Warmup::Wait
}

/// Whether a request to switch off should ramp: only while the cooler is on and the sensor is cold.
pub fn needs_ramp(cooler_on: bool, temp_c: Option<f64>) -> bool {
    cooler_on && temp_c.is_some_and(|t| t < WARM_ENOUGH_C)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cold_sensor_is_warmed_a_degree_at_a_time() {
        assert_eq!(warmup_step(-10.0, -10.0, Some(60.0), 30.0), Warmup::Raise(-9.0));
        // the target leads the sensor by at most 2 degrees
        assert_eq!(warmup_step(-10.0, -7.0, Some(50.0), 30.0), Warmup::Wait);
        assert_eq!(warmup_step(-8.0, -7.0, Some(50.0), 30.0), Warmup::Raise(-6.0));
    }

    #[test]
    fn it_finishes_when_warm_or_idle() {
        assert_eq!(warmup_step(15.0, 10.0, Some(30.0), 30.0), Warmup::Finish);
        assert_eq!(warmup_step(5.0, 8.0, Some(0.0), 30.0), Warmup::Finish);
        // at the highest target with the cooler still working: wait for the sensor
        assert_eq!(warmup_step(5.0, 30.0, Some(30.0), 30.0), Warmup::Wait);
    }

    #[test]
    fn only_a_cold_running_cooler_needs_the_ramp() {
        assert!(needs_ramp(true, Some(-5.0)));
        assert!(!needs_ramp(true, Some(20.0)));
        assert!(!needs_ramp(false, Some(-5.0)));
        assert!(!needs_ramp(true, None)); // unknown: no ramp possible, a plain off
    }
}
