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

/// What a camera accepts for a region: the multiples its SDK wants and the smallest window it reads out.
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct RoiLimits {
    pub step_x: u32,
    pub step_y: u32,
    pub step_w: u32,
    pub step_h: u32,
    pub min_w: u32,
    pub min_h: u32,
}

impl Default for RoiLimits {
    fn default() -> Self { RoiLimits { step_x: 1, step_y: 1, step_w: 1, step_h: 1, min_w: 16, min_h: 16 } }
}

/// Turns a requested region into one the camera takes: the size is rounded DOWN to the steps, the corner to the NEAREST step, and then the
/// corner is moved so the window lies inside the sensor. Refuses a region that is empty, larger than the sensor or below the minimum
/// size (after rounding), so a request is never quietly turned into something very different.
pub fn fit_roi(req: Roi, sensor: (u32, u32), lim: &RoiLimits) -> std::result::Result<Roi, String> {
    let step = |v: u32| v.max(1);
    let down = |v: u32, st: u32| v / step(st) * step(st);
    let nearest = |v: u32, st: u32| (v + step(st) / 2) / step(st) * step(st);
    let (min_w, min_h) = (lim.min_w.max(step(lim.step_w)), lim.min_h.max(step(lim.step_h)));
    let max_w = down(sensor.0, lim.step_w);
    let max_h = down(sensor.1, lim.step_h);
    if req.width < min_w || req.height < min_h {
        return Err(format!("the region must be at least {min_w} x {min_h} pixels"));
    }
    if req.width > sensor.0 || req.height > sensor.1 {
        return Err(format!("the region is larger than the sensor ({} x {})", sensor.0, sensor.1));
    }
    if req.x.saturating_add(req.width) > sensor.0 || req.y.saturating_add(req.height) > sensor.1 {
        return Err(format!("the region lies outside the sensor ({} x {})", sensor.0, sensor.1));
    }
    let w = down(req.width, lim.step_w).clamp(min_w, max_w.max(min_w));
    let h = down(req.height, lim.step_h).clamp(min_h, max_h.max(min_h));
    let x = nearest(req.x, lim.step_x).min(down(sensor.0 - w, lim.step_x));
    let y = nearest(req.y, lim.step_y).min(down(sensor.1 - h, lim.step_y));
    Ok(Roi { x, y, width: w, height: h })
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
    /// The multiples and minimum size this camera takes for a region (see [`fit_roi`]).
    fn roi_limits(&self) -> RoiLimits { RoiLimits::default() }
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

    const ASI: RoiLimits = RoiLimits { step_x: 2, step_y: 2, step_w: 8, step_h: 2, min_w: 64, min_h: 64 };
    const S: (u32, u32) = (3008, 3008);
    fn r(x: u32, y: u32, width: u32, height: u32) -> Roi { Roi { x, y, width, height } }

    #[test]
    fn size_rounds_down_and_the_corner_to_the_nearest_step() {
        assert_eq!(fit_roi(r(13, 7, 1503, 1501), S, &ASI).unwrap(), r(14, 8, 1496, 1500));
        assert_eq!(fit_roi(r(0, 0, 3008, 3008), S, &ASI).unwrap(), r(0, 0, 3008, 3008));
    }

    #[test]
    fn rounding_never_leaves_the_sensor_and_a_request_past_an_edge_is_refused() {
        // nearest corner would be 2999 -> 3000, but 3000 + 8 is past the edge: moved back inside
        let right = fit_roi(r(2999, 100, 9, 100), S, &RoiLimits { min_w: 8, ..ASI }).unwrap();
        assert_eq!(right, r(3000, 100, 8, 100));
        let right = fit_roi(r(2511, 100, 497, 500), S, &ASI).unwrap();
        assert_eq!(right, r(2512, 100, 496, 500));
        let bottom = fit_roi(r(100, 2745, 200, 263), S, &ASI).unwrap();
        assert_eq!((bottom.y, bottom.height), (2746, 262));
        assert!(bottom.y + bottom.height <= S.1);
        // all four edges, flush
        assert_eq!(fit_roi(r(0, 0, 64, 64), S, &ASI).unwrap(), r(0, 0, 64, 64));
        assert_eq!(fit_roi(r(2944, 0, 64, 64), S, &ASI).unwrap(), r(2944, 0, 64, 64));
        assert_eq!(fit_roi(r(0, 2944, 64, 64), S, &ASI).unwrap(), r(0, 2944, 64, 64));
        assert_eq!(fit_roi(r(2944, 2944, 64, 64), S, &ASI).unwrap(), r(2944, 2944, 64, 64));
        // past an edge
        assert!(fit_roi(r(2900, 100, 500, 500), S, &ASI).unwrap_err().contains("outside"));
        assert!(fit_roi(r(100, 2900, 100, 200), S, &ASI).unwrap_err().contains("outside"));
        assert!(fit_roi(r(100, 2990, 200, 18), S, &ASI).unwrap_err().contains("at least"));
    }

    #[test]
    fn odd_sensors_and_tiny_or_huge_requests() {
        let odd = (1281, 961);
        let o = fit_roi(r(0, 0, 1281, 961), odd, &ASI).unwrap();
        assert_eq!((o.width, o.height), (1280, 960));
        assert!(o.x + o.width <= 1281 && o.y + o.height <= 961 && o.x % 2 == 0 && o.y % 2 == 0);
        assert!(fit_roi(r(0, 0, 63, 100), S, &ASI).unwrap_err().contains("at least 64"));
        assert!(fit_roi(r(0, 0, 4000, 100), S, &ASI).unwrap_err().contains("larger than the sensor"));
        assert!(fit_roi(r(5000, 0, 100, 100), S, &ASI).unwrap_err().contains("outside"));
        assert!(fit_roi(r(0, 0, 0, 0), S, &RoiLimits::default()).is_err());
        assert_eq!(fit_roi(r(3, 5, 17, 19), S, &RoiLimits::default()).unwrap(), r(3, 5, 17, 19));
    }
}
