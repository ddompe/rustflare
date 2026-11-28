//! The settings a master dark or flat was taken with ("calibration key"), and what differs from the camera's state now.
//!
//! A dark only helps when it was taken with the same camera, region (size *and* position: hot pixels and vignetting belong to the
//! place on the sensor), bit depth, offset, gain and exposure, and at about the same sensor temperature. A flat depends on the camera,
//! region, bit depth, gain and the whole optical train (dust and vignetting), so it also names the optical setup. The key is written
//! into the master's note when it is taken, so it travels with the file; the comparison names each thing that differs.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Degrees Celsius a dark may differ from the sensor's current temperature before it is called different.
pub const TEMP_TOLERANCE_C: f64 = 3.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CalKey {
    /// Camera name as the driver reports it.
    pub camera: String,
    /// Region of the sensor: x, y, width, height.
    pub roi: Option<[u32; 4]>,
    pub exposure_us: Option<f64>,
    pub gain: Option<f64>,
    pub offset: Option<f64>,
    /// The camera was set to 16-bit output.
    pub bit16: Option<bool>,
    pub temp_c: Option<f64>,
    /// The named optical setup in use, when there is one.
    pub setup: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Dark,
    Flat,
}

fn close(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => (a - b).abs() <= 0.01 * a.abs().max(b.abs()).max(1.0),
        _ => true, // not known on one side: nothing to compare
    }
}

/// What differs between the settings a master was taken with and the camera's settings now, in words; empty when it fits.
pub fn differences(taken: &CalKey, now: &CalKey, kind: Kind) -> Vec<&'static str> {
    let mut d = Vec::new();
    if taken.camera != now.camera {
        d.push("camera");
    }
    if taken.roi != now.roi && taken.roi.is_some() && now.roi.is_some() {
        d.push("region");
    }
    if taken.bit16 != now.bit16 && taken.bit16.is_some() && now.bit16.is_some() {
        d.push("bit depth");
    }
    if !close(taken.gain, now.gain) {
        d.push("gain");
    }
    match kind {
        Kind::Dark => {
            if !close(taken.offset, now.offset) {
                d.push("offset");
            }
            if !close(taken.exposure_us, now.exposure_us) {
                d.push("exposure");
            }
            if let (Some(a), Some(b)) = (taken.temp_c, now.temp_c) {
                if (a - b).abs() > TEMP_TOLERANCE_C {
                    d.push("temperature");
                }
            }
        }
        Kind::Flat => {
            if let (Some(a), Some(b)) = (&taken.setup, &now.setup) {
                if a != b {
                    d.push("optical setup");
                }
            }
        }
    }
    d
}

/// Reads the key from the note beside a master (`<stem>_master.png` has `<stem>.json`, with the key under `details.calibration_key`).
pub fn read_from_note(note: &Value) -> Option<CalKey> {
    serde_json::from_value(note.get("details")?.get("calibration_key")?.clone()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> CalKey {
        CalKey { camera: "ZWO ASI533MC Pro".into(), roi: Some([1248, 1248, 512, 512]), exposure_us: Some(8000.0), gain: Some(100.0), offset: Some(30.0), bit16: Some(true), temp_c: Some(18.0), setup: Some("c11-barlow2x".into()) }
    }

    #[test]
    fn the_same_settings_differ_in_nothing() {
        assert!(differences(&key(), &key(), Kind::Dark).is_empty());
        assert!(differences(&key(), &key(), Kind::Flat).is_empty());
    }

    #[test]
    fn a_dark_names_every_setting_that_differs() {
        let mut now = key();
        now.roi = Some([0, 0, 512, 512]); // the same size somewhere else on the sensor
        now.offset = Some(10.0);
        now.bit16 = Some(false);
        now.temp_c = Some(-10.0);
        assert_eq!(differences(&key(), &now, Kind::Dark), vec!["region", "bit depth", "offset", "temperature"]);
    }

    #[test]
    fn a_warm_dark_within_the_tolerance_fits_and_a_flat_ignores_offset_exposure_and_temperature() {
        let mut now = key();
        now.temp_c = Some(20.5);
        assert!(differences(&key(), &now, Kind::Dark).is_empty());
        now.offset = Some(5.0);
        now.exposure_us = Some(1.0);
        now.temp_c = Some(-20.0);
        assert!(differences(&key(), &now, Kind::Flat).is_empty());
    }

    #[test]
    fn a_flat_follows_the_optical_setup_and_the_camera() {
        let mut now = key();
        now.setup = Some("c11-native".into());
        assert_eq!(differences(&key(), &now, Kind::Flat), vec!["optical setup"]);
        now.camera = "Player One Apollo-M MINI".into();
        assert_eq!(differences(&key(), &now, Kind::Flat), vec!["camera", "optical setup"]);
    }

    #[test]
    fn what_is_not_known_on_one_side_is_not_a_difference() {
        let mut taken = key();
        taken.temp_c = None;
        taken.setup = None;
        taken.roi = None;
        let mut now = key();
        now.temp_c = Some(-10.0);
        assert!(differences(&taken, &now, Kind::Dark).is_empty());
        assert!(differences(&taken, &now, Kind::Flat).is_empty());
    }

    #[test]
    fn the_key_is_read_back_from_a_note() {
        let note = serde_json::json!({ "details": { "calibration_key": key() } });
        assert_eq!(read_from_note(&note), Some(key()));
        assert_eq!(read_from_note(&serde_json::json!({ "details": {} })), None);
    }
}
