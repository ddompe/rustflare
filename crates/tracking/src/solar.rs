//! Solar angular size and image scale: lets limb tracking know the disc radius in pixels
//! from the optics (focal length, pixel size) instead of guessing it from a short arc.

/// Apparent equatorial position of the Sun (equinox of date) at `unix_secs`: (right ascension in hours, declination
/// in degrees). Low-precision formula from the Astronomical Almanac, good to about 0.01 degree, far finer than
/// the Sun's 0.5 degree disc.
pub fn sun_equatorial(unix_secs: f64) -> (f64, f64) {
    let n = unix_secs / 86400.0 + 2440587.5 - 2451545.0; // days since J2000.0
    let l = (280.460 + 0.9856474 * n).rem_euclid(360.0); // mean longitude
    let g = (357.528 + 0.9856003 * n).rem_euclid(360.0).to_radians(); // mean anomaly
    let lambda = (l + 1.915 * g.sin() + 0.020 * (2.0 * g).sin()).to_radians(); // ecliptic longitude
    let eps = (23.439 - 0.0000004 * n).to_radians();
    let ra = (eps.cos() * lambda.sin()).atan2(lambda.cos()).to_degrees().rem_euclid(360.0) / 15.0;
    let dec = (eps.sin() * lambda.sin()).asin().to_degrees();
    (ra, dec)
}

/// Altitude and azimuth in degrees (azimuth from north through east) of an equatorial position, for an observer at
/// `latitude_deg` / `longitude_east_deg` (east positive) at `unix_secs`.
pub fn alt_az(ra_hours: f64, dec_deg: f64, unix_secs: f64, latitude_deg: f64, longitude_east_deg: f64) -> (f64, f64) {
    let d = unix_secs / 86400.0 + 2440587.5 - 2451545.0;
    let gmst_h = (18.697374558 + 24.06570982441908 * d).rem_euclid(24.0);
    let ha = ((gmst_h + longitude_east_deg / 15.0 - ra_hours) * 15.0).to_radians();
    let (lat, dec) = (latitude_deg.to_radians(), dec_deg.to_radians());
    let alt = (lat.sin() * dec.sin() + lat.cos() * dec.cos() * ha.cos()).asin();
    let az = (-ha.sin() * dec.cos()).atan2(dec.sin() * lat.cos() - dec.cos() * lat.sin() * ha.cos());
    (alt.to_degrees(), az.to_degrees().rem_euclid(360.0))
}

/// Local sidereal time in hours for an observer at `longitude_east_deg` (east positive).
pub fn local_sidereal_hours(unix_secs: f64, longitude_east_deg: f64) -> f64 {
    let d = unix_secs / 86400.0 + 2440587.5 - 2451545.0;
    (18.697374558 + 24.06570982441908 * d + longitude_east_deg / 15.0).rem_euclid(24.0)
}

/// Mean solar angular radius at 1 AU, in arcseconds.
const RADIUS_AT_1AU_ARCSEC: f64 = 959.63;

/// Apparent solar radius (arcseconds) on the given day of the year (1 = Jan 1).
/// Uses the Earth–Sun distance from a first-order orbit model (perihelion ≈ Jan 3); accurate to ~0.1 %.
pub fn angular_radius_arcsec(day_of_year: u32) -> f64 {
    let distance_au = 1.0 - 0.01672 * (std::f64::consts::TAU * (day_of_year as f64 - 3.0) / 365.256).cos();
    RADIUS_AT_1AU_ARCSEC / distance_au
}

/// Image scale in arcseconds per (binned) pixel.
pub fn plate_scale_arcsec_per_px(focal_length_mm: f64, pixel_size_um: f64, binning: u32) -> f64 {
    206.265 * pixel_size_um * binning.max(1) as f64 / focal_length_mm
}

/// Expected solar radius in pixels.
pub fn expected_radius_px(focal_length_mm: f64, pixel_size_um: f64, binning: u32, day_of_year: u32) -> f64 {
    angular_radius_arcsec(day_of_year) / plate_scale_arcsec_per_px(focal_length_mm, pixel_size_um, binning)
}

/// Effective focal length (mm) implied by a measured solar radius in pixels: the inverse of `expected_radius_px`.
pub fn focal_from_radius(radius_px: f64, pixel_size_um: f64, binning: u32, day_of_year: u32) -> f64 {
    206.265 * pixel_size_um * binning.max(1) as f64 * radius_px / angular_radius_arcsec(day_of_year)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Seconds since the Unix epoch for a UTC date and time (days-from-civil algorithm).
    fn unix(y: i64, m: i64, d: i64, h: i64, mi: i64) -> f64 {
        let (y2, m2) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
        let era = y2.div_euclid(400);
        let yoe = y2 - era * 400;
        let doy = (153 * m2 + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        ((era * 146097 + doe - 719468) * 86400 + h * 3600 + mi * 60) as f64
    }

    #[test]
    fn sun_position_matches_known_dates() {
        // March equinox 2025 (20 Mar 09:01 UTC): RA ~ 0 h, Dec ~ 0
        let (ra, dec) = sun_equatorial(unix(2025, 3, 20, 9, 1));
        assert!(dec.abs() < 0.1, "dec {dec}");
        assert!(ra < 0.02 || ra > 23.98, "ra {ra}");
        // June solstice 2025 (21 Jun 02:42 UTC): Dec ~ +23.44, RA ~ 6 h
        let (ra, dec) = sun_equatorial(unix(2025, 6, 21, 2, 42));
        assert!((dec - 23.44).abs() < 0.05 && (ra - 6.0).abs() < 0.03, "ra {ra} dec {dec}");
        // 21 Nov 2026 12:00 UTC against an independent ephemeris (PyEphem, apparent place of date):
        // RA 15.7981 h, Dec -19.974 deg. Agreement here is ~15 arcseconds.
        let (ra, dec) = sun_equatorial(unix(2026, 11, 21, 12, 0));
        assert!((ra - 15.7981).abs() < 0.003, "ra {ra}");
        assert!((dec + 19.974).abs() < 0.02, "dec {dec}");
    }

    #[test]
    fn sun_is_up_at_local_noon_and_down_at_midnight() {
        // Observer at 40 N, 105 W (UTC-7): local solar noon around 19:00 UTC in spring
        let noon = unix(2025, 4, 10, 19, 0);
        let (ra, dec) = sun_equatorial(noon);
        let (alt, az) = alt_az(ra, dec, noon, 40.0, -105.0);
        assert!(alt > 50.0 && (az - 180.0).abs() < 20.0, "alt {alt} az {az}");
        let midnight = unix(2025, 4, 11, 7, 0);
        let (ra, dec) = sun_equatorial(midnight);
        let (alt, _) = alt_az(ra, dec, midnight, 40.0, -105.0);
        assert!(alt < -30.0, "alt {alt}");
    }

    #[test]
    fn radius_varies_with_season() {
        let jan = angular_radius_arcsec(3);
        let jul = angular_radius_arcsec(185);
        assert!((jan - 975.6).abs() < 1.0, "{jan}");
        assert!((jul - 943.6).abs() < 1.0, "{jul}");
    }

    #[test]
    fn focal_from_measured_radius_round_trips() {
        let r = expected_radius_px(1172.0, 4.5, 1, 325);
        assert!((focal_from_radius(r, 4.5, 1, 325) - 1172.0).abs() < 1e-6);
    }

    #[test]
    fn radius_in_pixels() {
        // 3.76 µm pixels at 1000 mm: 0.775"/px; the Sun (≈ 960") is ≈ 1239 px in radius.
        let r = expected_radius_px(1000.0, 3.76, 1, 80);
        assert!((r - 1237.0).abs() < 15.0, "{r}");
        // binning 2 halves it
        assert!((expected_radius_px(1000.0, 3.76, 2, 80) - r / 2.0).abs() < 1e-9);
    }
}
