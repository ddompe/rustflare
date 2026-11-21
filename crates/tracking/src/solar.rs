//! Solar angular size and image scale: lets limb tracking know the disc radius in pixels
//! from the optics (focal length, pixel size) instead of guessing it from a short arc.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radius_varies_with_season() {
        let jan = angular_radius_arcsec(3);
        let jul = angular_radius_arcsec(185);
        assert!((jan - 975.6).abs() < 1.0, "{jan}");
        assert!((jul - 943.6).abs() < 1.0, "{jul}");
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
