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
    /// The cooler of a cooled camera (`available: false` and nothing else for an uncooled one).
    pub cooler: CoolerView,
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
    /// How bright the brightest part of the frame is: the 99.99th percentile of the sampled pixels, 0..1 of full scale. A planet covers
    /// a tenth of a percent of the frame, so `mean` and `saturated` cannot see it; the planet profile judges clipping by this.
    pub peak: f64,
    /// 64-bin normalised histogram over full scale.
    pub histogram: Vec<f32>,
    /// Colour (Bayer) cameras only: normalised R, G and B histograms of the raw mosaic, `[r, g, b]`, 64 bins each. Absent for mono.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub histogram_rgb: Option<[Vec<f32>; 3]>,
    /// Colour cameras only: fraction of each channel's photosites at full scale, `[r, g, b]`. `saturated` is the largest of them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saturated_rgb: Option<[f64; 3]>,
}

/// What the Camera tab shows about the cooler: the camera's own report plus the sensor temperature and whether a gentle switch-off is running.
#[derive(Clone, Serialize, Default)]
pub struct CoolerView {
    pub available: bool,
    pub on: bool,
    pub target_c: Option<f64>,
    pub target_min_c: Option<f64>,
    pub target_max_c: Option<f64>,
    pub power_pct: Option<f64>,
    pub temp_c: Option<f64>,
    pub fan: Option<bool>,
    pub heater: Option<bool>,
    /// The target is being raised step by step before the cooler is switched off (see `cooler.rs`).
    pub warming_up: bool,
}

#[derive(Clone, Serialize)]
pub struct MountStatus {
    /// Whether the Pi sends its site and time by itself when it connects to a mount whose clock or place is off (setting `auto_sync_site_time`).
    pub auto_sync_site_time: bool,
    pub info: String,
    /// Whether the mount is tracking, as last read back from the mount itself.
    pub tracking: bool,
    /// The tracking rate in use or last chosen: solar, lunar or sidereal.
    pub rate: rustflare_core::TrackingRate,
    pub slewing: Option<bool>,
    pub at_home: Option<bool>,
    pub park: Option<rustflare_core::ParkState>,
    pub alt_az: Option<bool>,
    /// `usb` or `ble`, and whether that link is alive (a Bluetooth link can drop; the engine then drops the mount).
    pub link: Option<&'static str>,
    pub link_up: bool,
    /// The manual slew speeds this mount offers (multiples of sidereal), from the driver: the UI builds its list from them.
    pub slew_speeds: Vec<f64>,
    /// Whether the driver can search for the mechanical home position (`POST /api/mount/search-home`).
    pub can_search_home: bool,
    /// The site and clock the mount itself holds against the Pi's (issue #104). `None` when the driver cannot read them or nothing was read yet.
    pub site_time: Option<crate::sitetime::SiteTimeStatus>,
}

/// Something the user should know about that stays on screen until they dismiss it: a fallback, a lost frame, a failed stack.
#[derive(Clone, Serialize)]
pub struct Notice {
    pub id: u64,
    /// `error` (data may be lost) or `warning` (something did not go as asked, nothing is lost).
    pub level: &'static str,
    pub text: String,
    pub at: String,
    pub acked: bool,
}

/// What happens after each burst (docs/guide/recording-modes.md), and whether the stacker is there to do it.
#[derive(Clone, Serialize, Default)]
pub struct AfterBurstStatus {
    pub mode: String,
    pub stack_keep_percent: f64,
    pub raw_keep_percent: f64,
    pub margin: f64,
    pub stacker_connected: bool,
    /// Bursts handed to the stacker that have no answer yet.
    pub stacking: usize,
    /// Stacks made from memory since the program started, and the latest one.
    pub stacked: u32,
    pub last: Option<String>,
    /// Seconds the stacker needs per frame (measured), to plan with.
    pub stack_s_per_frame: f64,
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

#[derive(Clone, Default, Serialize)]
pub struct DirectIoStatus {
    /// The setting.
    pub wanted: bool,
    /// Used for the recording that is running or was last started.
    pub active: bool,
    pub problem: Option<String>,
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
    /// Direct writes (O_DIRECT) for recordings: the setting, and whether the last recording used them.
    pub direct_io: DirectIoStatus,
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
    /// Apparent radius of what the profile images (the Sun's, the Moon's from its distance, a planet's globe), arcseconds. None for `other`.
    pub target_radius_arcsec: Option<f64>,
    /// Expected solar radius in pixels (needs focal length and pixel size).
    pub expected_radius_px: Option<f64>,
    /// Radius measured from the limb when it disagrees with the optics, and the effective focal length and
    /// Barlow/reducer factor that would explain it (shown as a suggestion: the optics may simply be mis-entered).
    pub measured_radius_px: Option<f64>,
    pub suggested_effective_focal_mm: Option<f64>,
    pub suggested_multiplier: Option<f64>,
    /// The named optical setup in use, and whether the optics or the profile were changed after it was saved.
    pub setup: SetupStatus,
}

#[derive(Clone, Serialize, Default)]
pub struct SetupStatus {
    pub active: Option<String>,
    pub modified: bool,
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

#[derive(Clone, Serialize)]
pub struct TargetStatus {
    pub id: &'static str,
    pub name: &'static str,
}

#[derive(Clone, Serialize)]
pub struct FlipTargetStatus {
    pub id: &'static str,
    pub name: &'static str,
    pub ra_hours: f64,
    pub dec_deg: f64,
    /// Hour angle in hours (negative: before the meridian) and minutes until the target crosses it (negative: since), at the target's own rate.
    pub hour_angle_h: f64,
    pub minutes_to_meridian: f64,
    /// The tracking rate restored after a flip.
    pub rate: rustflare_core::TrackingRate,
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
    /// What differs between the settings the flat was taken with and the camera's now (empty when it fits or when not known).
    pub differs: Vec<&'static str>,
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
    /// Mean, saturation and histogram of the frame (one pass).
    pub stats_ms: f64,
    /// The tracker's measurement and the guider's step.
    pub track_ms: f64,
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
    /// Multiples and minimum the camera takes, so the UI snaps the same way the engine does.
    pub limits: rustflare_core::RoiLimits,
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
    /// What differs between the settings the dark was taken with and the camera's now: camera, region, bit depth, gain, offset, exposure, temperature.
    pub differs: Vec<&'static str>,
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
    /// The error of that score (standard error of the mean of the frames that counted): differences below it are noise.
    pub lucky_err: Option<f64>,
    /// The peak of the smoothed history since the session started or was reset, its error, and how long ago it was reached.
    pub best: Option<f64>,
    pub best_err: Option<f64>,
    pub best_age_s: Option<f64>,
    /// (seconds since start, lucky score, error) per burst of about 2.5 s, oldest first.
    pub history: Vec<[f64; 3]>,
    pub note: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct CoronagraphStatus {
    pub enabled: bool,
    pub scale: f64,
}

#[derive(Clone, Serialize)]
pub struct Status {
    /// The software version, and for a local build the commit it was built from (`0.7.1 (local 48fb5bc)`): shown in a corner of every screen.
    pub version: String,
    pub coronagraph: CoronagraphStatus,
    pub focus: FocusStatus,
    pub colour: ColourStatus,
    /// What is being imaged (`sun_white`, `sun_ha`, `moon`, `planet`, `other`): the UI adapts its words and what it enables, the engine names the target in the FITS headers.
    pub profile: &'static str,
    pub darks: DarksStatus,
    pub perf: PerfStatus,
    /// Region of interest of the open camera (None: no camera or not supported).
    pub roi: Option<RoiStatus>,
    pub session_log: SessionLogStatus,
    pub guide_params: GuideParamsStatus,
    pub flats: FlatsStatus,
    pub site: Option<SiteStatus>,
    pub sky_sun: Option<SkySun>,
    /// What the profile images (None for `other`, and for the planet profile until a planet was gone to).
    pub target: Option<TargetStatus>,
    /// The body the meridian flip follows (the Sun for the Sun profiles; the Moon, or the planet last gone to): where it is and when it
    /// crosses the meridian. For the Sun it is the same numbers as `sky_sun`. Needs a saved site.
    pub flip_target: Option<FlipTargetStatus>,
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
    pub after_burst: AfterBurstStatus,
    pub notices: Vec<Notice>,
    /// Last asynchronous error, shown by the UI until a new action succeeds.
    pub error: Option<String>,
    pub seq: u64,
}

impl Default for Status {
    fn default() -> Self {
        Status {
            version: build_version(),
            session_log: SessionLogStatus { lost_frames: true, lost_saved: 0, enabled: false, lines: 0, frames: 0, bytes: 0, file: None },
            roi: None,
            perf: PerfStatus::default(),
            darks: DarksStatus { available: false, applied: false, name: None, exposure_us: None, gain: None, matches_camera: false, differs: vec![] },
            coronagraph: CoronagraphStatus { enabled: false, scale: 1.03 },
            focus: FocusStatus::default(),
            colour: ColourStatus { mode: "auto", r: 1.0, b: 1.0, applied: [1.0; 3] },
            profile: "sun_ha",
            guide_params: GuideParamsStatus { window_s: 10.0, deadband_px: 3.0, gain: 0.7, speed: 0.2, fast_speed: 0.8, fast_above_px: 20.0 },
            flats: FlatsStatus { available: false, applied: false, name: None, differs: vec![] },
            site: None,
            sky_sun: None,
            target: None,
            flip_target: None,
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
            capture: CaptureStatus { state: "idle", phase: String::new(), done: 0, total: None, file: None, last_file: None, burst: None, bursts_total: None, next_in_s: None, dropped: 0, buffer_queued: 0, buffer_capacity: 0, buffer_mb: 1024, ram_available_mb: None, disk_mb_s: 0.0, paused: false, paused_reason: None, stopped_early: None, frames_lost: None, eta_s: None, direct_io: DirectIoStatus::default() },
            after_burst: AfterBurstStatus::default(),
            notices: Vec::new(),
            error: None,
            seq: 0,
        }
    }
}

/// `0.7.1`, or `0.7.1 (local 48fb5bc)` when the build script says it is a local build (`RUSTFLARE_BUILD`).
pub fn build_version() -> String {
    match option_env!("RUSTFLARE_BUILD") {
        Some(b) if !b.is_empty() => format!("{} ({b})", env!("CARGO_PKG_VERSION")),
        _ => env!("CARGO_PKG_VERSION").to_string(),
    }
}

/// The use profiles, in the order the UI offers them.
pub const PROFILES: [&str; 5] = ["sun_white", "sun_ha", "moon", "planet", "other"];

/// The FITS `OBJECT` of a profile (`None`: do not claim a target).
pub fn profile_object(profile: &str) -> Option<String> {
    match profile {
        "sun_white" | "sun_ha" => Some("Sun".into()),
        "moon" => Some("Moon".into()),
        "planet" => Some("Planet".into()),
        _ => None,
    }
}

#[cfg(test)]
mod profile_tests {
    use super::*;

    #[test]
    fn every_profile_names_its_target_or_none() {
        assert_eq!(profile_object("sun_ha").as_deref(), Some("Sun"));
        assert_eq!(profile_object("moon").as_deref(), Some("Moon"));
        assert_eq!(profile_object("other"), None);
        assert!(PROFILES.iter().all(|p| *p == "other" || profile_object(p).is_some()));
    }
}
