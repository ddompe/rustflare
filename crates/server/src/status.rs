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
    /// Colour filter layout of a one-shot-colour camera ("RGGB" ...); frames are raw mosaics. None for mono.
    pub bayer: Option<&'static str>,
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
    /// The target was lost while guiding: pulses are paused and guiding resumes by itself when the limb is found again.
    pub held: bool,
    /// The Sun cannot be seen, but guiding goes on from the drift measured while it could.
    pub dead_reckoning: bool,
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
    /// Frames dropped because the RAM buffer was full (only with the "drop" policy; 0 otherwise).
    pub dropped: u64,
    pub buffer_queued: usize,
    pub buffer_capacity: usize,
    pub buffer_mb: u64,
    pub ram_available_mb: Option<u64>,
    pub disk_mb_s: f64,
    /// A recording is paused (by you or for a meridian flip) and can be resumed, and why.
    pub paused: bool,
    pub paused_reason: Option<String>,
    /// Why the last recording stopped on its own (drive removed, disk full, write error) and what was lost. It stays until the
    /// next recording starts, unlike `error`, which any later command clears.
    pub stopped_early: Option<String>,
    /// Set when a finished recording lost frames (drop mode, the drive could not keep up): says how many, in which file; kept until the next recording starts.
    pub frames_lost: Option<String>,
    /// Seconds until the running recording ends, as far as can be told (None: it runs until stopped).
    pub eta_s: Option<f64>,
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

/// Observer site (east-positive longitude), as saved on the Pi.
#[derive(Clone, Serialize)]
pub struct SiteStatus {
    pub latitude: f64,
    pub longitude: f64,
}

/// Where the Sun is in the sky now, for the saved site (shown next to Go to Sun).
#[derive(Clone, Serialize)]
pub struct SkySun {
    pub altitude_deg: f64,
    pub azimuth_deg: f64,
    pub ra_hours: f64,
    pub dec_deg: f64,
    /// Hour angle in hours (negative: before the meridian) and minutes until the Sun crosses it (negative: since).
    pub hour_angle_h: f64,
    pub minutes_to_meridian: f64,
}

/// The Moon and bright planets for the saved site (the targets of Go to besides the Sun).
#[derive(Clone, Serialize)]
pub struct SkyTarget {
    pub id: &'static str,
    pub name: &'static str,
    pub altitude_deg: f64,
    pub azimuth_deg: f64,
    pub ra_hours: f64,
    pub dec_deg: f64,
}

/// The Pi's clock decides the Sun's computed position and what the mount is told the time is.
#[derive(Clone, Serialize, Default)]
pub struct ClockStatus {
    /// `systemd` says the clock is synchronised to a time server (None where that cannot be read).
    pub ntp_synchronized: Option<bool>,
}

#[derive(Clone, Serialize)]
pub struct FlipStatus {
    pub phase: &'static str,
    pub elapsed_s: f64,
    pub detail: String,
}

#[derive(Clone, Serialize)]
pub struct AutoFlipStatus {
    pub enabled: bool,
    pub record_min: f64,
    pub flip_min: f64,
    /// off, waiting (for the meridian), recorded (position saved), flipping, done (for this crossing)
    pub stage: &'static str,
    /// What is going on, in words, for the status bar and the overlay (empty when there is nothing worth saying).
    pub message: String,
    /// A recording was paused for the flip and starts again afterwards.
    pub resume_pending: bool,
}

#[derive(Clone, Serialize)]
pub struct GotoStatus {
    pub target: &'static str,
    /// "starting" until the mount reports motion, then "slewing".
    pub phase: &'static str,
}

#[derive(Clone, Serialize)]
pub struct FlatsStatus {
    pub available: bool,
    /// The live view is flat-corrected right now.
    pub applied: bool,
    pub name: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct GuideParamsStatus {
    pub window_s: f64,
    pub deadband_px: f64,
    pub gain: f64,
    pub speed: f64,
    pub fast_speed: f64,
    pub fast_above_px: f64,
}

#[derive(Clone, Serialize)]
pub struct SessionLogStatus {
    /// Saving the frame whenever tracking is lost, and how many were saved since the service started.
    pub lost_frames: bool,
    pub lost_saved: u32,
    pub enabled: bool,
    pub lines: u64,
    pub frames: u64,
    pub bytes: u64,
    pub file: Option<String>,
}

/// Where the engine thread spends its time (smoothed milliseconds per occurrence), for performance work.
#[derive(Clone, Serialize, Default)]
pub struct PerfStatus {
    /// Whole `on_frame` (every camera frame).
    pub frame_ms: f64,
    /// The ~10 Hz analysis block: statistics, histogram, Sun location, tracking.
    pub analysis_ms: f64,
    pub locate_ms: f64,
    /// Cloning the frame for the preview thread and for `last_frame`.
    pub clone_ms: f64,
    /// Recording path (`step_job`).
    pub job_ms: f64,
    pub publish_ms: f64,
}

#[derive(Clone, Serialize)]
pub struct RoiStatus {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub sensor_width: u32,
    pub sensor_height: u32,
}

#[derive(Clone, Serialize)]
pub struct DarksStatus {
    pub available: bool,
    pub applied: bool,
    pub name: Option<String>,
    pub exposure_us: Option<f64>,
    pub gain: Option<f64>,
    /// The camera's current exposure and gain equal the ones the dark was taken at (otherwise it does not help).
    pub matches_camera: bool,
}

#[derive(Clone, Serialize)]
pub struct ColourStatus {
    pub mode: &'static str,
    pub r: f64,
    pub b: f64,
    /// Gains in effect (red, green, blue).
    pub applied: [f64; 3],
}

/// Focus assist: live and lucky sharpness scores and their history while the user turns the focuser.
#[derive(Clone, Serialize, Default)]
pub struct FocusStatus {
    pub enabled: bool,
    /// Smoothed score of the latest frames (immediate feedback while turning).
    pub live: Option<f64>,
    /// Lucky score of the last finished burst (mean of its sharpest quarter).
    pub lucky: Option<f64>,
    /// Best lucky score since the session started or was reset, and how long ago it was reached.
    pub best: Option<f64>,
    pub best_age_s: Option<f64>,
    /// (seconds since start, lucky score) per burst, oldest first.
    pub history: Vec<[f64; 2]>,
    pub note: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct CoronagraphStatus {
    pub enabled: bool,
    pub scale: f64,
}

#[derive(Clone, Serialize)]
pub struct Status {
    pub coronagraph: CoronagraphStatus,
    pub focus: FocusStatus,
    pub colour: ColourStatus,
    pub darks: DarksStatus,
    pub perf: PerfStatus,
    /// Region of interest of the open camera (None: no camera or not supported).
    pub roi: Option<RoiStatus>,
    pub session_log: SessionLogStatus,
    pub guide_params: GuideParamsStatus,
    pub flats: FlatsStatus,
    pub site: Option<SiteStatus>,
    pub sky_sun: Option<SkySun>,
    pub sky_targets: Vec<SkyTarget>,
    pub clock: ClockStatus,
    pub goto: Option<GotoStatus>,
    pub flip: Option<FlipStatus>,
    pub auto_flip: AutoFlipStatus,
    /// The last recorded flip (pre-flip pointing and Sun position; after a flip also its duration and result).
    pub last_flip: Option<serde_json::Value>,
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
            session_log: SessionLogStatus { lost_frames: true, lost_saved: 0, enabled: false, lines: 0, frames: 0, bytes: 0, file: None },
            roi: None,
            perf: PerfStatus::default(),
            darks: DarksStatus { available: false, applied: false, name: None, exposure_us: None, gain: None, matches_camera: false },
            coronagraph: CoronagraphStatus { enabled: false, scale: 1.03 },
            focus: FocusStatus::default(),
            colour: ColourStatus { mode: "auto", r: 1.0, b: 1.0, applied: [1.0; 3] },
            guide_params: GuideParamsStatus { window_s: 10.0, deadband_px: 3.0, gain: 0.7, speed: 0.2, fast_speed: 0.8, fast_above_px: 20.0 },
            flats: FlatsStatus { available: false, applied: false, name: None },
            site: None,
            sky_sun: None,
            sky_targets: Vec::new(),
            clock: ClockStatus::default(),
            goto: None,
            flip: None,
            auto_flip: AutoFlipStatus { enabled: false, record_min: 5.0, flip_min: 1.0, stage: "off", message: String::new(), resume_pending: false },
            last_flip: None,
            startup: StartupStatus::default(),
            optics: OpticsStatus::default(),
            camera: None,
            sun: None,
            mount: None,
            tracking: None,
            guiding: GuidingStatus { state: "off", calibration: None, target: None, error_px: None, held: false, dead_reckoning: false },
            capture: CaptureStatus { state: "idle", phase: String::new(), done: 0, total: None, file: None, last_file: None, burst: None, bursts_total: None, next_in_s: None, dropped: 0, buffer_queued: 0, buffer_capacity: 0, buffer_mb: 1024, ram_available_mb: None, disk_mb_s: 0.0, paused: false, paused_reason: None, stopped_early: None, frames_lost: None, eta_s: None },
            error: None,
            seq: 0,
        }
    }
}
