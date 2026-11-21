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

/// An opened camera. Drivers are blocking; the server calls them from a dedicated thread.
pub trait Camera: Send {
    fn info(&self) -> CameraInfo;
    fn controls(&mut self) -> Result<Vec<ControlDesc>>;
    fn set_control(&mut self, id: &str, value: f64) -> Result<()>;
    /// Blocks until a frame (exposure + readout) is available or `timeout` elapses.
    fn capture(&mut self, timeout: Duration) -> Result<Frame>;
    fn temperature_c(&mut self) -> Option<f64> { None }
    /// Physical pixel size in micrometres, if the driver knows it (used to compute the image scale).
    fn pixel_size_um(&self) -> Option<f64> { None }
}

pub trait CameraDriver: Send + Sync {
    fn name(&self) -> &'static str;
    fn enumerate(&self) -> Result<Vec<CameraInfo>>;
    fn open(&self, id: &str) -> Result<Box<dyn Camera>>;
}
