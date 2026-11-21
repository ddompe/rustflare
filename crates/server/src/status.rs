use rustflare_core::ControlDesc;
use rustflare_tracking::Calibration;
use serde::Serialize;

#[derive(Clone, Serialize, Default)]
pub struct CameraStatus {
    pub driver: String,
    pub id: String,
    pub name: String,
    pub controls: Vec<ControlDesc>,
    pub temperature_c: Option<f64>,
    pub width: u32,
    pub height: u32,
    pub bits: u32,
    pub fps: f64,
    /// Mean level, 0..1 of full scale.
    pub mean: f64,
    /// Fraction of (subsampled) pixels at full scale: clipping indicator.
    pub saturated: f64,
    /// 64-bin normalised histogram over full scale.
    pub histogram: Vec<f32>,
}

#[derive(Clone, Serialize)]
pub struct MountStatus {
    pub info: String,
    /// Whether the mount is tracking, as last read back from the mount itself.
    pub tracking: bool,
    pub slewing: Option<bool>,
    pub at_home: Option<bool>,
    pub park: Option<rustflare_core::ParkState>,
    pub alt_az: Option<bool>,
    /// `usb` or `ble`, and whether that link is alive (a Bluetooth link can drop; the engine then drops the mount).
    pub link: Option<&'static str>,
    pub link_up: bool,
}

#[derive(Clone, Serialize)]
pub struct TrackingStatus {
    pub mode: &'static str,
    pub x: f64,
    pub y: f64,
    pub radius: Option<f64>,
    /// Target lost (e.g. no disc in frame); the tracker keeps retrying.
    pub lost: bool,
}

#[derive(Clone, Serialize)]
pub struct GuidingStatus {
    /// "off" | "calibrating" | "guiding"
    pub state: &'static str,
    pub calibration: Option<Calibration>,
    pub target: Option<(f64, f64)>,
    pub error_px: Option<f64>,
}

#[derive(Clone, Serialize)]
pub struct CaptureStatus {
    /// "idle" | "single" | "sequence" | "flats"
    pub state: &'static str,
    /// Human-readable phase, e.g. "finding exposure (3)".
    pub phase: String,
    pub done: u32,
    pub total: Option<u32>,
    pub file: Option<String>,
    pub last_file: Option<String>,
    /// Bursts: current burst number, total (None = until stopped), seconds until the next one starts.
    pub burst: Option<u32>,
    pub bursts_total: Option<u32>,
    pub next_in_s: Option<f64>,
    /// RAM buffer: frames queued for the disk / capacity, configured size, memory available, disk speed.
    pub buffer_queued: usize,
    pub buffer_capacity: usize,
    pub buffer_mb: u64,
    pub ram_available_mb: Option<u64>,
    pub disk_mb_s: f64,
}

#[derive(Clone, Serialize, Default)]
pub struct OpticsStatus {
    /// Native focal length of the telescope.
    pub focal_length_mm: Option<f64>,
    /// Barlow (> 1) / reducer (< 1) factor, and the resulting effective focal length.
    pub focal_multiplier: f64,
    pub effective_focal_mm: Option<f64>,
    /// Manual pixel-size override (µm); `None` = use the camera's.
    pub pixel_size_override_um: Option<f64>,
    /// Pixel size actually used, and where it came from ("camera" | "manual").
    pub pixel_size_um: Option<f64>,
    pub pixel_size_source: Option<&'static str>,
    pub binning: u32,
    pub scale_arcsec_per_px: Option<f64>,
    /// Apparent solar radius today, arcseconds.
    pub solar_radius_arcsec: f64,
    /// Expected solar radius in pixels (needs focal length and pixel size).
    pub expected_radius_px: Option<f64>,
    /// Radius measured from the limb when it disagrees with the optics, and the effective focal length and
    /// Barlow/reducer factor that would explain it (shown as a suggestion: the optics may simply be mis-entered).
    pub measured_radius_px: Option<f64>,
    pub suggested_effective_focal_mm: Option<f64>,
    pub suggested_multiplier: Option<f64>,
}

#[derive(Clone, Serialize, Default)]
pub struct StartupStatus {
    /// Reconnect the remembered camera and mount automatically.
    pub enabled: bool,
    pub camera: Option<String>,
    pub mount: Option<String>,
    /// Currently trying to reconnect (e.g. just after boot).
    pub retrying: bool,
}

#[derive(Clone, Serialize)]
pub struct Status {
    pub startup: StartupStatus,
    pub optics: OpticsStatus,
    pub camera: Option<CameraStatus>,
    /// Sun location in the latest frame, whether or not tracking is on (centring aid).
    pub sun: Option<rustflare_tracking::SunLocation>,
    pub mount: Option<MountStatus>,
    pub tracking: Option<TrackingStatus>,
    pub guiding: GuidingStatus,
    pub capture: CaptureStatus,
    /// Last asynchronous error, shown by the UI until a new action succeeds.
    pub error: Option<String>,
    pub seq: u64,
}

impl Default for Status {
    fn default() -> Self {
        Status {
            startup: StartupStatus::default(),
            optics: OpticsStatus::default(),
            camera: None,
            sun: None,
            mount: None,
            tracking: None,
            guiding: GuidingStatus { state: "off", calibration: None, target: None, error_px: None },
            capture: CaptureStatus { state: "idle", phase: String::new(), done: 0, total: None, file: None, last_file: None, burst: None, bursts_total: None, next_in_s: None, buffer_queued: 0, buffer_capacity: 0, buffer_mb: 1024, ram_available_mb: None, disk_mb_s: 0.0 },
            error: None,
            seq: 0,
        }
    }
}
