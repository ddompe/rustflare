//! Meridian flip bookkeeping: where things were before the flip, where they should be after, and how the image and the
//! guiding calibration change when the mount (and with it the camera) turns over.
//!
//! After a flip the camera has turned half a revolution about the optical axis, so the image is rotated by 180 degrees:
//! a point at (x, y) in a `w` x `h` frame appears at (w - x, h - y), and moving an axis in the same direction as before
//! now moves the image the opposite way, i.e. both calibration directions are negated.

use crate::Calibration;
use rustflare_core::{Mat2, Vec2};

/// Where a frame position ends up after the image is rotated by 180 degrees.
pub fn flipped_position(p: Vec2, width: f64, height: f64) -> Vec2 {
    Vec2::new(width - p.x, height - p.y)
}

/// The guiding calibration for the flipped image (both axis directions reversed, rates unchanged).
pub fn flipped_calibration(c: &Calibration) -> Option<Calibration> {
    let p = Vec2::new(-c.primary_dir.x, -c.primary_dir.y);
    let s = Vec2::new(-c.secondary_dir.x, -c.secondary_dir.y);
    Some(Calibration { primary_dir: p, secondary_dir: s, img_to_axes: Mat2::from_cols(p, s).invert()?, rate: c.rate })
}

/// Which orientation the image has after the flip, decided from where the Sun actually appears: the mount may or may
/// not have turned over, so the target is whichever of "unchanged" and "rotated" is closer to the observed position.
/// Returns (target, rotated).
pub fn choose_target(before: Vec2, observed: Vec2, width: f64, height: f64) -> (Vec2, bool) {
    let rotated = flipped_position(before, width, height);
    if (observed - rotated).len() < (observed - before).len() { (rotated, true) } else { (before, false) }
}

/// Hour angle of the Sun in hours (-12..12; negative = before the meridian) from its right ascension and the local
/// sidereal time, and the minutes until (negative: since) it crosses the meridian, using the Sun's own day length.
pub fn meridian_timing(ra_hours: f64, lst_hours: f64) -> (f64, f64) {
    let mut ha = (lst_hours - ra_hours).rem_euclid(24.0);
    if ha > 12.0 {
        ha -= 24.0;
    }
    (ha, -ha * 60.0 * 1.002738)
}

/// Hour angle in hours and minutes to the meridian for a body whose hour angle grows by `rate` hours per hour of time (see
/// `bodies::hour_angle_rate`). Negative minutes: it crossed that long ago.
pub fn meridian_timing_at_rate(ra_hours: f64, lst_hours: f64, rate: f64) -> (f64, f64) {
    let (ha, _) = meridian_timing(ra_hours, lst_hours);
    (ha, -ha * 60.0 / rate.max(0.1))
}

/// A pointing, then a pointing after the flip with the aim of landing in the same place.
/// Target of the flip GoTo: where the user pointed, carried along by the Sun's own motion in the meantime.
pub fn carried_target(pointing: (f64, f64), sun_then: (f64, f64), sun_now: (f64, f64)) -> (f64, f64) {
    ((pointing.0 + (sun_now.0 - sun_then.0)).rem_euclid(24.0), (pointing.1 + (sun_now.1 - sun_then.1)).clamp(-90.0, 90.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_image_rotates_a_half_turn() {
        let p = flipped_position(Vec2::new(300.0, 200.0), 1000.0, 800.0);
        assert_eq!((p.x, p.y), (700.0, 600.0));
        let back = flipped_position(p, 1000.0, 800.0);
        assert_eq!((back.x, back.y), (300.0, 200.0));
    }

    #[test]
    fn calibration_flips_sign_and_still_inverts() {
        let c = Calibration::from_motion(Vec2::new(10.0, 1.0), Vec2::new(-1.0, 10.0), 4.0, 0.5).unwrap();
        let f = flipped_calibration(&c).unwrap();
        assert!((f.primary_dir.x + c.primary_dir.x).abs() < 1e-12 && (f.secondary_dir.y + c.secondary_dir.y).abs() < 1e-12);
        // a pixel offset needs the opposite axis command after the flip
        let off = Vec2::new(30.0, -20.0);
        let (a, b) = (c.guiding_direction(off).unwrap(), f.guiding_direction(off).unwrap());
        assert!((a.x + b.x).abs() < 1e-9 && (a.y + b.y).abs() < 1e-9);
        assert_eq!(f.rate, c.rate);
    }

    #[test]
    fn the_target_follows_what_the_image_actually_did() {
        let before = Vec2::new(300.0, 200.0);
        let (t, rot) = choose_target(before, Vec2::new(690.0, 610.0), 1000.0, 800.0);
        assert!(rot && (t.x, t.y) == (700.0, 600.0));
        let (t, rot) = choose_target(before, Vec2::new(310.0, 190.0), 1000.0, 800.0);
        assert!(!rot && (t.x, t.y) == (300.0, 200.0), "the mount did not turn over: keep the old position");
    }

    #[test]
    fn meridian_timing_signs() {
        assert!((meridian_timing(10.0, 9.5).0 + 0.5).abs() < 1e-9, "Sun still east: negative hour angle");
        let (ha, mins) = meridian_timing(10.0, 10.25);
        assert!(ha > 0.0 && mins < 0.0, "past the meridian");
        assert!((meridian_timing(23.9, 0.1).0 - 0.2).abs() < 1e-9, "wraps over midnight");
    }

    #[test]
    fn the_moon_reaches_the_meridian_later_than_the_hour_angle_alone_says() {
        let (ha, mins) = meridian_timing_at_rate(10.0, 9.0, 0.966);
        assert!((ha + 1.0).abs() < 1e-9);
        assert!((mins - 60.0 / 0.966).abs() < 1e-9, "{mins}");
        assert!(mins > 62.0, "later than the 60 minutes of a body that keeps pace with the clock");
    }

    #[test]
    fn carried_target_moves_with_the_sun() {
        let t = carried_target((15.80, -19.90), (15.800, -19.900), (15.8005, -19.8995));
        assert!((t.0 - 15.8005).abs() < 1e-9 && (t.1 + 19.8995).abs() < 1e-9);
        assert!((carried_target((23.99, 0.0), (23.99, 0.0), (0.01, 0.0)).0 - 0.0).abs() < 0.03, "wraps");
    }
}
