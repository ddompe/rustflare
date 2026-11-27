use crate::{Frame, Result};
use serde::Serialize;
use std::time::Duration;

/// Well-known control ids every driver should use where applicable.
pub const CTRL_EXPOSURE_US: &str = "exposure_us";
pub const CTRL_GAIN: &str = "gain";
pub const CTRL_OFFSET: &str = "offset";
/// 0 = 8-bit, 1 = 16-bit output.
pub const CTRL_HIGH_BIT_DEPTH: &str = "high_bit_depth";

#[derive(Clone, Debug, Serialize)]
pub struct CameraInfo {
    pub driver: String,
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ControlDesc {
    pub id: String,
    pub label: String,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub value: f64,
    pub unit: Option<String>,
    /// Display on a logarithmic slider (exposure, gain).
    pub log: bool,
}

/// What a cooled camera reports about its thermoelectric cooler. `None` from [`Camera::cooler`] means no cooler.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct CoolerStatus {
    pub on: bool,
    /// Target sensor temperature in degrees Celsius, whole degrees.
    pub target_c: Option<f64>,
    /// The range the camera accepts for the target.
    pub target_min_c: Option<f64>,
    pub target_max_c: Option<f64>,
    /// Cooler power, 0 to 100 percent (read only).
    pub power_pct: Option<f64>,
    /// `None` when the camera has no switch for it.
    pub fan: Option<bool>,
    pub heater: Option<bool>,
}

/// A change to the cooler: every field that is `Some` is applied (the target first, then the switches).
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize)]
pub struct CoolerRequest {
    pub on: Option<bool>,
    pub target_c: Option<f64>,
    pub fan: Option<bool>,
    pub heater: Option<bool>,
}

/// Region of interest in sensor pixels (top-left origin).
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Roi {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// A centred region covering `fraction` (0..=1) of the sensor in each direction, rounded down to multiples of 8
/// (width) and 4 (height) with an even offset, which every camera we know accepts.
pub fn centred_roi(sensor: (u32, u32), fraction: f64) -> Roi {
    let f = fraction.clamp(0.05, 1.0);
    let w = (((sensor.0 as f64 * f) as u32) / 8 * 8).clamp(8, sensor.0 / 8 * 8);
    let h = (((sensor.1 as f64 * f) as u32) / 4 * 4).clamp(4, sensor.1 / 4 * 4);
    Roi { x: (sensor.0 - w) / 2 / 2 * 2, y: (sensor.1 - h) / 2 / 2 * 2, width: w, height: h }
}

/// An opened camera. Drivers are blocking; the server calls them from a dedicated thread.
pub trait Camera: Send {
    fn info(&self) -> CameraInfo;
    fn controls(&mut self) -> Result<Vec<ControlDesc>>;
    fn set_control(&mut self, id: &str, value: f64) -> Result<()>;
    /// Blocks until a frame (exposure + readout) is available or `timeout` elapses.
    fn capture(&mut self, timeout: Duration) -> Result<Frame>;
    /// Colour filter layout when the sensor is a one-shot-colour one (frames are then raw Bayer mosaics).
    fn bayer(&self) -> Option<crate::BayerPattern> { None }
    fn temperature_c(&mut self) -> Option<f64> { None }
    /// The cooler of a cooled camera (state and what it can do), or `None` when there is none.
    fn cooler(&mut self) -> Option<CoolerStatus> { None }
    /// Changes the cooler, target temperature, fan or anti-dew heater. Cameras without them return `Unsupported`.
    fn set_cooler(&mut self, _req: &CoolerRequest) -> Result<()> {
        Err(crate::Error::Unsupported("cooler on this camera".into()))
    }
    /// Physical pixel size in micrometres, if the driver knows it (used to compute the image scale).
    fn pixel_size_um(&self) -> Option<f64> { None }
    /// Full sensor size, if the driver can report it (needed for ROI).
    fn sensor_size(&self) -> Option<(u32, u32)> { None }
    /// The current region of interest (the whole sensor when none is set).
    fn roi(&self) -> Option<Roi> { None }
    /// Reads out only `roi`: smaller frames, higher frame rate. Drivers that cannot do it return `Unsupported`.
    fn set_roi(&mut self, _roi: Roi) -> Result<()> {
        Err(crate::Error::Unsupported("region of interest on this camera".into()))
    }
}

pub trait CameraDriver: Send + Sync {
    fn name(&self) -> &'static str;
    fn enumerate(&self) -> Result<Vec<CameraInfo>>;
    fn open(&self, id: &str) -> Result<Box<dyn Camera>>;
}

#[cfg(test)]
mod roi_tests {
    use super::*;

    #[test]
    fn centred_regions_are_aligned_and_inside() {
        let r = centred_roi((1944, 1472), 0.5);
        assert_eq!((r.width, r.height), (968, 736));
        assert!(r.x % 2 == 0 && r.y % 2 == 0 && r.x + r.width <= 1944 && r.y + r.height <= 1472);
        assert_eq!(centred_roi((1944, 1472), 1.0), Roi { x: 0, y: 0, width: 1944, height: 1472 });
        let q = centred_roi((1944, 1472), 0.25);
        assert_eq!((q.width % 8, q.height % 4), (0, 0));
    }
}
