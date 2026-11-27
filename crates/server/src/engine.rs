//! The engine owns every piece of hardware and all capture state on one thread.
//! HTTP handlers talk to it with [`Command`]s and read published [`Status`] (ADR 0004).

use crate::preview;
use crate::storage::Storage;
use crate::status::*;
use rustflare_camera as cams;
use rustflare_capture::{read_png, write_png, BurstSpec, FlatAccumulator, FlatDecision, FlatTarget, Format, FrameSink, SerWriter};
use rustflare_core::camera::CTRL_EXPOSURE_US;
use rustflare_core::sim::SimSky;
use crate::stackbridge::{Bridge, Event as BridgeEvent};
use crate::status::AfterBurstStatus;
use rustflare_capture::Resolution;
use rustflare_core::{Axis, Camera, CameraDriver, Error, Frame, Mount, MountState, ParkState, RadPerSec, SiteTime, SlewSpeed, TrackingRate, Vec2, SIDEREAL_RATE};
use rustflare_tracking::*;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{oneshot, watch};

pub type Reply = Result<Value, String>;

/// What to reconnect at startup. Saved in the settings file under `startup`.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Startup {
    pub enabled: bool,
    pub camera: Option<(String, String)>,         // (driver, id)
    pub mount: Option<(String, Option<String>)>,  // (driver, serial device)
}

impl Default for Startup {
    fn default() -> Self { Startup { enabled: true, camera: None, mount: None } }
}

/// Observer site, saved in the settings file under `site`. Longitude is east-positive.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Site {
    pub latitude: f64,
    pub longitude: f64,
}

/// What was recording, to start it again after an automatic meridian flip paused it.
#[derive(Clone)]
enum ResumeSpec {
    Bursts { name: Option<String>, frames: Option<u32>, seconds: Option<f64>, interval_s: Option<f64>, bursts: Option<u32>, format: Option<String>, on_full: Option<String>, home_when_done: bool },
    Lanes { name: Option<String>, kind: Option<String>, cycle_interval_s: Option<f64>, lanes: Vec<LaneSpec>, cycles: Option<u32>, format: Option<String>, on_full: Option<String>, home_when_done: bool },
}

/// Unattended meridian flip, saved in the settings file under `auto_flip`.
/// `record_min`: minutes before the meridian at which the pointing and the Sun's place in the picture are recorded.
/// `flip_min`: minutes after the meridian at which the flip starts (sooner if the mount has stopped tracking by itself).
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct AutoFlip {
    pub enabled: bool,
    pub record_min: f64,
    pub flip_min: f64,
}
impl Default for AutoFlip {
    fn default() -> Self { AutoFlip { enabled: false, record_min: 5.0, flip_min: 1.0 } }
}

/// A meridian flip assist in progress: GoTo to the recorded pointing, then re-centre the Sun where it was.
struct FlipState {
    started: Instant,
    pre: Value,
    moved: bool,
    last_check: Instant,
    slew_done_at: Option<Instant>,
    slew_s: Option<f64>,
    phase: &'static str,
    /// Image position to centre on and whether the image turned over, once known.
    centre: Option<((f64, f64), bool)>,
    good_since: Option<Instant>,
    /// Guiding was running when the flip started: calibrate again afterwards (the axes may now point the other way).
    was_guiding: bool,
    cal_gen: u32,
    /// The tracking rate to restore after the turn: the target's (solar for the Sun, lunar for the Moon, sidereal for a planet).
    rate: TrackingRate,
}

/// Focus assist session: scores frames, summarises one-second bursts, keeps the history for the graph.
struct FocusState {
    started: Instant,
    last_scored: Instant,
    burst_started: Instant,
    burst: Vec<f64>,
    live: Option<f64>,
    /// (seconds since start, lucky score, its error) per burst, oldest first.
    history: Vec<[f64; 3]>,
    /// The scoring window, fixed in the frame when the session starts (and whether the Sun was known then).
    window: Option<(rustflare_tracking::focus::Window, bool)>,
}

/// Length of a focus burst: long enough to hold some frames of good seeing, short enough to follow the focuser.
const FOCUS_BURST: Duration = Duration::from_millis(2500);

struct GotoState {
    started: Instant,
    /// The mount has been seen slewing.
    moved: bool,
    last_check: Instant,
    /// What it is going to, and the tracking rate to start on arrival (solar, lunar or sidereal).
    target: &'static str,
    rate: TrackingRate,
}

/// Optical setup, persisted in the settings file; gives the expected solar radius in pixels.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct OpticsSettings {
    /// Native focal length of the telescope, before any Barlow or reducer.
    pub focal_length_mm: Option<f64>,
    /// Barlow (> 1) or focal reducer (< 1) factor; the effective focal length is `focal_length_mm * focal_multiplier`.
    pub focal_multiplier: f64,
    /// Manual pixel size (µm); when unset the camera's own value is used.
    pub pixel_size_um: Option<f64>,
    pub binning: u32,
}

impl Default for OpticsSettings {
    fn default() -> Self { OpticsSettings { focal_length_mm: None, focal_multiplier: 1.0, pixel_size_um: None, binning: 1 } }
}

pub enum Command {
    ListCameras,
    OpenCamera { driver: String, id: String },
    CloseCamera,
    SetControl { id: String, value: f64 },
    /// Cooler on/off, target temperature, fan, anti-dew heater. Switching off a cold sensor ramps unless `immediate`.
    SetCooler { req: rustflare_core::CoolerRequest, immediate: bool },
    ConnectMount { driver: String, device: Option<String> },
    DisconnectMount,
    MountTracking { enabled: bool, rate: Option<TrackingRate> },
    MountSlew { axis: Axis, speed: f64 },
    MountStop,
    /// Moves the mount to its home (zero) position; the API asks for explicit confirmation.
    MountHome,
    /// Finds the mount's mechanical zero with its homing sensors (iOptron `:MSH#`) and makes it the new zero. Moves the mount.
    MountSearchHome,
    /// Sync site and time, slew to the target's current position, then switch tracking on at the target's rate. Moves the mount.
    /// `target`: "sun" (the default) or a body id (moon, mars, jupiter, saturn). Below the horizon only with `allow_below_horizon`.
    MountGoto { target: String, allow_unsynced_clock: bool, allow_below_horizon: bool },
    /// Observer site (decimal degrees, east-positive longitude).
    SetSite { latitude: f64, longitude: f64 },
    /// Meridian flip assist. `record_only`: just remember the pointing and Sun position. `restore`: the flip was done by
    /// hand, GoTo the recorded pointing and re-centre. Otherwise: record, GoTo (the mount turns over if it decides to), re-centre.
    StartFlip { record_only: bool, restore: bool },
    /// Debugging: engine internals as JSON.
    DebugState,
    /// Debugging: save the latest frame and a note now; returns the path under the storage folder.
    DebugSnapshot,
    /// Save the frame and a note whenever tracking is lost (debugging).
    SetLostFrames { enabled: bool },
    /// Automatic meridian flip on/off and its timing (minutes before the meridian to record, minutes after to flip).
    SetAutoFlip { enabled: bool, record_min: f64, flip_min: f64 },
    /// Centred region of interest covering `fraction` of the sensor in each direction (1.0 = full frame).
    SetRoi { fraction: f64 },
    /// Opt-in session log (JSON lines plus small frames) for regression fixtures.
    SetSessionLog { enabled: bool, frame_every_s: Option<f64> },
    /// Disc frame plus a `factor` times longer exposure for prominences, and their composite.
    CaptureProminence { name: Option<String>, factor: f64 },
    /// Master dark from `frames` frames at the current exposure and gain.
    CaptureDarks { name: Option<String>, frames: u32 },
    SetDarkApplied { enabled: bool },
    /// Prominence view: mask the Sun's disc in the live view (display only).
    SetCoronagraph { enabled: bool, scale: f64 },
    /// What is being imaged: one of `status::PROFILES`.
    SetProfile { id: String },
    /// White balance of a colour camera: mode off / auto / manual, manual red and blue gains (green = 1).
    SetColour { mode: String, r: f64, b: f64 },
    /// Focus assist on or off; `reset` clears the history and the best score.
    SetFocus { enabled: bool, reset: bool },
    /// Guiding tuning; takes effect the next time guiding starts.
    SetGuideParams { window_s: f64, deadband_px: f64, gain: f64, speed: f64, fast_speed: f64, fast_above_px: f64 },
    /// Switch flat-field correction of the live view on or off (a master flat must exist).
    SetFlatApplied { enabled: bool },
    StartTracking { mode: String, x: Option<f64>, y: Option<f64>, radius: Option<f64> },
    StopTracking,
    Calibrate,
    /// `center`: steer the target to the frame centre instead of holding the current position.
    StartGuiding { center: bool },
    StopGuiding,
    CaptureSingle { name: Option<String>, format: Option<String> },
    /// One burst (`interval_s` = None) or a schedule of bursts, recorded through the RAM buffer.
    /// `frames` / `seconds`: when a burst ends. `interval_s`: start-to-start spacing. `bursts`: None = until stopped.
    /// HDR bracketing: cycle through lanes (own exposure, own directory); `cycles` None = until stopped.
    CaptureLanes { name: Option<String>, kind: Option<String>, cycle_interval_s: Option<f64>, lanes: Vec<LaneSpec>, cycles: Option<u32>, format: Option<String>, on_full: Option<String>, home_when_done: bool },
    CaptureBursts { name: Option<String>, frames: Option<u32>, seconds: Option<f64>, interval_s: Option<f64>, bursts: Option<u32>, format: Option<String>, on_full: Option<String>, home_when_done: bool },
    SetBuffer { mb: u64 },
    SetAfterBurst { patch: Value },
    /// Write recordings with O_DIRECT (skipping the page cache) instead of through it; see `rustflare_capture::set_direct_io`.
    SetDirectIo { enabled: bool },
    /// A read-only question to the mount (see `Mount::query`), for diagnosing it.
    MountQuery { what: String },
    AckNotices { id: Option<u64> },
    /// Reconnect the last camera and mount automatically at startup (and after a camera error).
    SetStartup { enabled: bool },
    CaptureFlats { name: Option<String>, frames: u32, level: Option<f64> },
    CaptureStop,
    /// Stops writing but remembers what was recording (with the cycles or bursts that are left): `CaptureResume` carries on.
    CapturePause,
    CaptureResume,
    SetOptics { focal_length_mm: Option<f64>, focal_multiplier: f64, pixel_size_um: Option<f64>, binning: u32 },
    /// The named optical setups: `{ active, modified, setups }`.
    ListSetups,
    /// Saves a setup under `name` from `body` (missing fields come from what is in use now).
    SaveSetup { name: String, body: Value },
    DeleteSetup { name: String },
    /// Makes a setup the current optics and profile and re-selects its flat and dark. Moves and starts nothing.
    ApplySetup { name: String },
}

struct Request {
    cmd: Command,
    reply: oneshot::Sender<Reply>,
}

/// Cloneable handle used by the HTTP layer.
#[derive(Clone)]
pub struct Handle {
    tx: mpsc::Sender<Request>,
    pub status: watch::Receiver<Status>,
    pub preview: watch::Receiver<Option<Arc<Vec<u8>>>>,
}

impl Handle {
    pub async fn call(&self, cmd: Command) -> Reply {
        let (reply, rx) = oneshot::channel();
        self.tx.send(Request { cmd, reply }).map_err(|_| "engine stopped".to_string())?;
        rx.await.map_err(|_| "engine stopped".to_string())?
    }
}

/// What the guider is given on a frame: a measured position, only the time (a pulse may need to end), or the order to
/// judge what was collected (the end of a burst of measurements in a lane recording).
enum GuideInput {
    Pos(Vec2),
    Tick,
    Judge,
}

enum GuideState {
    Off,
    Calibrating(Calibrator),
    Guiding(Guider),
}

enum CapPhase {
    Waiting { next_at: Instant },
    Capturing { stem: PathBuf, started: Instant, taken: u32 },
}

/// A sequence (one burst) or a schedule of bursts. Frames go through `sink`: a RAM buffer drained to disk
/// by a writer thread, so slow storage never throttles the camera (ADR 0012).
/// One HDR bracket lane: an exposure setting captured into its own directory.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct LaneSpec {
    pub name: String,
    pub exposure_us: f64,
    pub gain: Option<f64>,
    pub frames: Option<u32>,
    pub seconds: Option<f64>,
    /// Coronagraph: black out the Sun's disc in the recorded frames of this lane (otherwise frames are raw).
    #[serde(default)]
    pub mask_disc: bool,
}

/// Lane cycling state of a capture job: lanes are visited in order, each burst goes to the next lane.
struct Lanes {
    specs: Vec<LaneSpec>,
    dir: PathBuf,
    original: (f64, Option<f64>),
    /// Start-to-start time of one pass through all lanes (time-lapse cadence); None = back to back.
    cycle_interval: Option<Duration>,
    cycle_started: Instant,
    /// Index of the lane being recorded.
    current: usize,
    /// Frames to discard after a setting change so the first kept frame has the new exposure.
    skip: u32,
}

struct CaptureJob {
    lanes: Option<Lanes>,
    name: String,
    kind: &'static str,
    format: Format,
    burst_frames: Option<u32>,
    burst_seconds: Option<f64>,
    /// Start-to-start spacing of bursts; `None` = a single burst.
    interval: Option<Duration>,
    total_bursts: Option<u32>,
    burst_no: u32,
    phase: CapPhase,
    /// Largest file the target filesystem can hold (FAT32: 4 GiB); SER files roll over before reaching it.
    max_file: Option<u64>,
    last_check: Instant,
    sink: FrameSink,
    /// RAM buffer full: drop the new frame (keeps the camera at full speed) instead of waiting for the disk.
    drop_when_full: bool,
    /// Which sink this is, so a stack job can find the ring it belongs to.
    sink_no: u64,
    /// The burst being captured is handed to the stacker (set at its start, taken at its end).
    cur: Option<CurStack>,
    /// The "stacker is not available" warning has been shown for this recording.
    warned_no_stacker: bool,
    /// Prominence lane with the coronagraph: where the Sun's model put the disc in each saved frame of the burst (x, y, radius; radius 0 = Sun not in view).
    /// The frames themselves stay raw; the stacker draws the black disc after stacking (docs/guide/stacking.md).
    disc_track: Vec<[f32; 3]>,
    /// Send the mount home when this recording finishes by itself (not when stopped, paused or failed).
    home_when_done: bool,
    /// Bursts finished so far and their total time, to estimate when the recording will end.
    bursts_done: u32,
    burst_time_sum: f64,
}

enum FlatPhase {
    Adjust { iter: u32, skip: u32 },
    Collect { writer: SerWriter, acc: FlatAccumulator },
}

struct Flats {
    stem: PathBuf,
    target: FlatTarget,
    frames: u32,
    phase: FlatPhase,
    original_exposure: f64,
    exposure_used: f64,
}

enum Job {
    Idle,
    Single { stem: PathBuf, fits: bool },
    Capture(Box<CaptureJob>),
    Flats(Box<Flats>),
    Prominence(Box<Prominence>),
    Darks(Box<Darks>),
}

/// Dark-frame capture: N frames at the current exposure and gain (cap on), averaged into a master dark.
struct Darks {
    stem: PathBuf,
    frames: u32,
    acc: FlatAccumulator,
    exposure_us: f64,
    gain: f64,
    skip: u32,
    /// The raw dark frames, kept next to the master so any stacker can build its own (raw data is the product).
    raw: Option<SerWriter>,
}

/// Two-exposure prominence capture: the disc at the current exposure, then a much longer one.
struct Prominence {
    stem: PathBuf,
    factor: f64,
    original_exposure: f64,
    long_exposure: f64,
    /// Disc frame and the Sun's position when it was taken.
    disc: Option<(Frame, (f64, f64, f64))>,
    /// Frames still to discard after the exposure change so the long frame really has the new exposure.
    skip: u32,
}

/// The lane whose frames show the disc properly: the shortest exposure among the lanes that are not coronagraph-masked.
fn reference_lane(specs: &[LaneSpec]) -> usize {
    specs.iter().enumerate().filter(|(_, l)| !l.mask_disc).min_by(|a, b| a.1.exposure_us.total_cmp(&b.1.exposure_us)).map_or(0, |(i, _)| i)
}

pub struct Engine {
    drivers: Vec<Box<dyn CameraDriver>>,
    sky: SimSky,
    camera: Option<Box<dyn Camera>>,
    mount: Option<Box<dyn Mount>>,
    mount_tracking: bool,
    /// The rate the mount tracks at (or will when tracking is switched on): the Sun's unless a Go to or the user chose another.
    mount_rate: TrackingRate,
    /// What the mount itself last reported (it can be changed from outside: its buttons, another app).
    mount_state: MountState,
    last_mount_poll: Instant,
    mount_poll_failures: u32,
    site: Option<Site>,
    guide_params: GuideParams,
    goto: Option<GotoState>,
    focus: Option<FocusState>,
    flip: Option<FlipState>,
    last_flip: Option<Value>,
    cal_generation: u32,
    /// Debug aid: keep the frame (and what the software knew) whenever tracking is lost. Setting `debug.lost_frames`, on by default.
    save_lost: bool,
    /// Last trustworthy Sun measurement (with the frame size): held for the mask and overlays while clouds hide the limb.
    sun_model: crate::sunmodel::SunModel,
    /// What the mount does on its own, learned while guiding works.
    drift_model: DriftModel,
    /// While the Sun cannot be seen: the guider is fed a dead-reckoned position so the framing holds.
    dead: Option<DeadReckoning>,
    /// Last trustworthy guide position: when, where, and the guider's applied motion then.
    last_good: Option<(Instant, Vec2, Vec2)>,
    /// A guide pulse is running (or just ended): the image moves on purpose, so the gate must follow the measurements.
    pulsing: bool,
    was_analysing: bool,
    /// Lane recordings: the last position seen in a long-exposure frame, to turn the next one into a displacement.
    lane_prev: Option<(Instant, Vec2)>,
    pulse_settle_until: Instant,
    manual_slew_until: Instant,
    /// The mount reports tracking off and no move of ours explains it: since when, and whether the user has been told.
    tracking_off_since: Option<Instant>,
    tracking_off_noticed: bool,
    sun_held: bool,
    /// Mean frame brightness (0..1) seen while the Sun was being measured well: what "good light" means for this setup.
    good_level: Option<f64>,
    /// When the tracker started failing (it is only called lost after a couple of seconds: shadows come and go).
    fail_since: Option<Instant>,
    fail_saved: bool,
    last_sun_event: Instant,
    /// Guiding that was running when the target was lost: target spot and calibration, resumed when the limb is found again.
    guide_hold: Option<(Vec2, Calibration)>,
    lost_saved: u32,
    last_lost_saved: Option<Instant>,
    last_exposure_change: Instant,
    /// A slow average of the picture's brightness while it was normal: when the picture falls far below it (a shadow over the telescope, thick cloud)
    /// the limb fit finds edges in noise and the Sun model followed them (2026-11-26: 400 px off in seconds, then 300 px of wrong corrections).
    bright_ref: f64,
    auto_flip: AutoFlip,
    /// Pointing and Sun position recorded shortly before the meridian by the automatic flip.
    auto_pre: Option<Value>,
    /// The last capture that was started, and (while an automatic flip has paused one) the one to start again afterwards.
    last_capture: Option<ResumeSpec>,
    resume_after_flip: Option<ResumeSpec>,
    resume_at: Option<Instant>,
    /// A recording paused by the user: what is left of it, and when it was paused.
    paused: Option<ResumeSpec>,
    trace_dir: Option<std::path::PathBuf>,
    last_prune: Option<Instant>,
    /// What the capture-state file last said and when (it is rewritten on a change and every 10 s as a heartbeat).
    state_file: Option<(String, Instant)>,
    /// File writes (event log, state file) happen on a low-priority thread, never on this one.
    alog: crate::asynclog::AsyncLog,
    /// Gaps between camera frames much longer than usual, to trace what stalled capture.
    last_gap_event: Option<Instant>,
    /// How long the engine spent on the previous frame (analysis, preview, pushing it to the recorder), to tell a stall in the
    /// camera's USB transfer from one in our own work (issue #86).
    last_frame_work: Duration,
    /// The automatic flip has been started (or given up) for this crossing of the meridian.
    auto_done: bool,
    /// The automatic flip was switched on after the moment to flip, with nothing recorded before it: it does nothing this crossing
    /// (the mount may already have been flipped by hand, and flipping it back would turn the Sun away).
    auto_late: bool,
    last_auto_check: Instant,
    last_auto_warn: Option<Instant>,
    ntp_sync: Option<bool>,
    last_ntp_check: Instant,
    /// A Bluetooth connection attempt running on its own thread (auto-reconnect must not stall the engine).
    pending_mount: Option<Receiver<Result<Box<dyn Mount>, String>>>,
    tracker: Option<Tracker>,
    lost: bool,
    guide: GuideState,
    calibration: Option<Calibration>,
    job: Job,
    /// Sink of a finished/aborted capture that is still writing its buffered frames to disk.
    draining: Option<FrameSink>,
    draining_no: u64,
    /// The finished recording's sink stays open until the bursts waiting for a stack are answered.
    close_when_stacked: bool,
    bridge: Bridge,
    after: AfterBurst,
    /// The user's choice of direct writes (saved); whether it is in use is decided per recording against the drive.
    direct_io: bool,
    stack_jobs: std::collections::HashMap<u64, StackJob>,
    job_no: u64,
    sink_counter: u64,
    ring_sent: Option<u64>,
    stack_s_per_frame: f64,
    stacked_count: u32,
    last_stack_note: Option<String>,
    notice_id: u64,
    buffer_mb: u64,
    buffer_cap: usize,
    /// Real write speed of the most recent finished capture (MB/s), kept after the writer is gone.
    last_disk_mb_s: f64,
    last_analysis: Instant,
    last_mean: f64,
    last_frame: Option<Frame>,
    /// When `last_frame` was taken: it is refreshed at most twice a second (a 5.7 MB copy), which is plenty for snapshots and flats.
    last_frame_saved: Instant,
    /// Counts analyses, to run the Sun detector now and then while the tracker is on.
    analysis_n: u32,
    storage: Arc<Storage>,
    optics: OpticsSettings,
    /// Named optical setups (trains), and which one is in use.
    setups: crate::setups::Setups,
    startup: Startup,
    /// Control values to re-apply when each camera is opened, keyed `driver/id`; written back lazily.
    camera_controls: serde_json::Map<String, Value>,
    controls_dirty: Option<Instant>,
    /// While set, keep trying to reconnect the remembered camera/mount (they may not be enumerated yet at boot).
    auto_until: Option<Instant>,
    last_auto_try: Instant,
    camera_pixel_um: Option<f64>,
    /// Expected solar radius in pixels (from optics), used to make partial-disc fits reliable.
    expected_radius: Option<f64>,
    /// The planet last gone to: what the planet profile's meridian flip follows.
    planet_target: Option<rustflare_tracking::bodies::Body>,
    /// Recent radii measured from the limb when the optics' radius did not fit the data (see `fit_limb_auto`).
    radius_samples: std::collections::VecDeque<f64>,
    /// Median of those, used as the radius hint once it is stable: it fixes the radius-centre degeneracy of short arcs.
    measured_radius: Option<f64>,
    status: Status,
    status_tx: watch::Sender<Status>,
    preview_tx: SyncSender<Frame>,
    flat: rustflare_capture::SharedFlat,
    corona: preview::SharedCorona,
    /// Auto-stretch to the brightest part of the frame instead of the 99.8th percentile (the planet profile: a planet covers well under 0.2 % of the frame).
    peak_stretch: Arc<std::sync::atomic::AtomicBool>,
    colour: preview::SharedColour,
    demand: Arc<preview::Demand>,
    slog: crate::sessionlog::SessionLog,
    last_preview: Instant,
    last_publish: Instant,
    last_temp: Instant,
    /// Set while a cold sensor is being warmed before the cooler goes off: when the target was last raised.
    cooler_warmup: Option<Instant>,
    fps_ema: f64,
    last_frame_at: Option<Instant>,
}

/// Starts the engine thread.
pub fn spawn(storage: Arc<Storage>, stretch: Arc<std::sync::atomic::AtomicBool>, demand: Arc<preview::Demand>) -> (Handle, std::thread::JoinHandle<()>) {
    let (tx, rx) = mpsc::channel();
    let guide_params = load_guide_params(&storage);
    let mut first_status = Status::default();
    first_status.guide_params = GuideParamsStatus { window_s: guide_params.window.as_secs_f64(), deadband_px: guide_params.deadband_px, gain: guide_params.gain, speed: guide_params.speed, fast_speed: guide_params.fast_speed, fast_above_px: guide_params.fast_above_px };
    first_status.profile = crate::config::read(storage.config_file()).get("profile").and_then(|v| v.as_str()).and_then(|v| crate::status::PROFILES.iter().find(|p| **p == v).copied()).unwrap_or("sun_ha");
    let peak_stretch = Arc::new(std::sync::atomic::AtomicBool::new(first_status.profile == "planet"));
    let (status_tx, status_rx) = watch::channel(first_status.clone());
    let (jpeg_tx, jpeg_rx) = watch::channel(None);
    let sky = SimSky::new();
    let corona: preview::SharedCorona = Default::default();
    let colour: preview::SharedColour = Default::default();
    {
        let c = crate::config::read(storage.config_file());
        if let Some(v) = c.get("colour") {
            let mut g = colour.lock().unwrap();
            g.mode = match v.get("mode").and_then(|m| m.as_str()) { Some("off") => "off", Some("manual") => "manual", _ => "auto" };
            g.r = v.get("r").and_then(|x| x.as_f64()).unwrap_or(1.0).clamp(0.25, 4.0);
            g.b = v.get("b").and_then(|x| x.as_f64()).unwrap_or(1.0).clamp(0.25, 4.0);
        }
    }
    {
        let c = crate::config::read(storage.config_file());
        let mut g = corona.lock().unwrap();
        g.enabled = c.get("coronagraph").and_then(|v| v.get("enabled")).and_then(|v| v.as_bool()).unwrap_or(false);
        g.scale = c.get("coronagraph").and_then(|v| v.get("scale")).and_then(|v| v.as_f64()).filter(|s| (0.8..=1.2).contains(s)).unwrap_or(1.03);
    }
    let flat: rustflare_capture::SharedFlat = Default::default();
    load_saved_flat(&storage, &flat);
    load_saved_dark(&storage, &flat);
    let engine = Engine {
        drivers: cams::drivers(sky.clone()),
        sky,
        camera: None,
        mount: None,
        mount_tracking: false,
        mount_rate: TrackingRate::Solar,
        mount_state: MountState::default(),
        last_mount_poll: Instant::now(),
        mount_poll_failures: 0,
        site: crate::config::read(storage.config_file()).get("site").and_then(|v| serde_json::from_value(v.clone()).ok()),
        goto: None,
        focus: None,
        flip: None,
        auto_flip: crate::config::read(storage.config_file()).get("auto_flip").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default(),
        cal_generation: 0,
        sun_model: { let mut m = crate::sunmodel::SunModel::default(); m.set_gate_floor(model_gate_floor(first_status.profile)); m },
        drift_model: DriftModel::default(),
        dead: None,
        last_good: None,
        pulsing: false,
        was_analysing: false,
        lane_prev: None,
        pulse_settle_until: Instant::now(),
        manual_slew_until: Instant::now(),
        tracking_off_since: None,
        tracking_off_noticed: false,
        sun_held: false,
        good_level: None,
        fail_since: None,
        fail_saved: false,
        last_sun_event: Instant::now() - Duration::from_secs(60),
        save_lost: crate::config::read(storage.config_file()).get("debug").and_then(|d| d.get("lost_frames")).and_then(|v| v.as_bool()).unwrap_or(true),
        lost_saved: 0,
        guide_hold: None,
        last_lost_saved: None,
        last_exposure_change: Instant::now(),
        bright_ref: 0.0,
        auto_pre: None,
        last_capture: None,
        resume_after_flip: None,
        resume_at: None,
        paused: None,
        trace_dir: None,
        last_prune: None,
        state_file: None,
        alog: crate::asynclog::AsyncLog::new(),
        last_gap_event: None,
        last_frame_work: Duration::ZERO,
        auto_done: false,
        auto_late: false,
        last_auto_check: Instant::now(),
        last_auto_warn: None,
        last_flip: crate::config::read(storage.config_file()).get("last_flip").cloned(),
        guide_params,
        ntp_sync: None,
        last_ntp_check: Instant::now() - Duration::from_secs(120),
        pending_mount: None,
        tracker: None,
        lost: false,
        guide: GuideState::Off,
        calibration: None,
        job: Job::Idle,
        draining: None,
        draining_no: 0,
        close_when_stacked: false,
        bridge: Bridge::start(&storage.config_file().with_file_name("stack.sock")),
        direct_io: crate::config::read(storage.config_file()).get("direct_io").and_then(|v| v.as_bool()).unwrap_or(false),
        after: AfterBurst::load(&crate::config::read(storage.config_file()).get("after_burst").cloned().unwrap_or(Value::Null)),
        stack_jobs: Default::default(),
        job_no: 0,
        sink_counter: 0,
        ring_sent: None,
        stack_s_per_frame: 0.06,
        stacked_count: 0,
        last_stack_note: None,
        notice_id: 0,
        buffer_mb: crate::config::read(storage.config_file()).get("capture").and_then(|c| c["buffer_mb"].as_u64()).unwrap_or(1024).clamp(64, 8192),
        buffer_cap: 0,
        last_disk_mb_s: 0.0,
        last_analysis: Instant::now() - Duration::from_secs(1),
        last_mean: 0.0,
        last_frame: None,
        last_frame_saved: Instant::now(),
        analysis_n: 0,
        optics: rustflare_config_optics(&storage),
        setups: crate::setups::Setups::load(storage.config_file()),
        startup: crate::config::read(storage.config_file()).get("startup").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default(),
        camera_controls: crate::config::read(storage.config_file()).get("camera_controls").and_then(|v| v.as_object().cloned()).unwrap_or_default(),
        controls_dirty: None,
        auto_until: Some(Instant::now() + Duration::from_secs(120)),
        last_auto_try: Instant::now() - Duration::from_secs(10),
        camera_pixel_um: None,
        expected_radius: None,
        planet_target: crate::config::read(storage.config_file()).get("planet_target").and_then(|v| v.as_str()).and_then(rustflare_tracking::bodies::Body::from_id),
        radius_samples: Default::default(),
        measured_radius: None,
        storage,
        status: first_status,
        status_tx,
        preview_tx: preview::spawn(jpeg_tx, stretch, peak_stretch.clone(), flat.clone(), corona.clone(), colour.clone()),
        peak_stretch,
        flat,
        corona,
        colour,
        demand,
        slog: Default::default(),
        last_preview: Instant::now(),
        last_publish: Instant::now(),
        last_temp: Instant::now(),
        cooler_warmup: None,
        fps_ema: 0.0,
        last_frame_at: None,
    };
    let jh = std::thread::Builder::new().name("engine".into()).spawn(move || engine.run(rx)).expect("spawn engine");
    (Handle { tx, status: status_rx, preview: jpeg_rx }, jh)
}

fn s<E: std::fmt::Display>(e: E) -> String { e.to_string() }

fn rustflare_config_optics(storage: &Storage) -> OpticsSettings {
    crate::config::read(storage.config_file()).get("optics").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

fn sanitize(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).take(40).collect()
}

/// Guiding tuning saved in the settings file (key `guide`); anything missing or out of range keeps the default.
fn load_guide_params(storage: &Storage) -> GuideParams {
    let mut p = GuideParams::default();
    if let Some(g) = crate::config::read(storage.config_file()).get("guide") {
        let f = |k: &str| g.get(k).and_then(|v| v.as_f64());
        if let Some(v) = f("window_s").filter(|v| (2.0..=60.0).contains(v)) { p.window = Duration::from_secs_f64(v); }
        if let Some(v) = f("deadband_px").filter(|v| (0.0..=30.0).contains(v)) { p.deadband_px = v; }
        if let Some(v) = f("gain").filter(|v| (0.05..=1.0).contains(v)) { p.gain = v; }
        if let Some(v) = f("speed").filter(|v| (0.1..=0.9).contains(v)) { p.speed = v; }
        if let Some(v) = f("fast_speed").filter(|v| (0.1..=0.9).contains(v)) { p.fast_speed = v; }
        if let Some(v) = f("fast_above_px").filter(|v| (2.0..=200.0).contains(v)) { p.fast_above_px = v; }
    }
    p
}

/// Loads a master flat from the storage folder (`rel` relative to it) and installs it.
fn load_flat(storage: &Storage, flat: &rustflare_capture::SharedFlat, rel: &str, enabled: bool) -> Result<(), String> {
    let m = crate::files::resolve(&storage.dir(), rel).and_then(|p| read_png(&p).map_err(|e| e.to_string()))?;
    let ff = rustflare_capture::FlatField::from_master(&m).map_err(|e| e.to_string())?;
    let mut s = flat.lock().unwrap();
    s.field = Some(std::sync::Arc::new(ff));
    s.enabled = enabled;
    s.name = Some(rel.to_string());
    Ok(())
}

/// Loads a master dark the same way; its exposure and gain are given when known.
fn load_dark(storage: &Storage, flat: &rustflare_capture::SharedFlat, rel: &str, enabled: bool, exposure_us: Option<f64>, gain: Option<f64>) -> Result<(), String> {
    let m = crate::files::resolve(&storage.dir(), rel).and_then(|p| read_png(&p).map_err(|e| e.to_string()))?;
    let df = rustflare_capture::DarkFrame::from_master(&m).map_err(|e| e.to_string())?;
    let mut s = flat.lock().unwrap();
    s.dark = Some(std::sync::Arc::new(df));
    s.dark_enabled = enabled;
    s.dark_name = Some(rel.to_string());
    s.dark_exposure_us = exposure_us;
    s.dark_gain = gain;
    Ok(())
}

/// Restores the master flat chosen last time (config key `flat`: master path relative to the storage folder).
fn load_saved_flat(storage: &Storage, flat: &rustflare_capture::SharedFlat) {
    let cfg = crate::config::read(storage.config_file());
    let Some(f) = cfg.get("flat") else { return };
    let (Some(rel), enabled) = (f.get("master").and_then(|v| v.as_str()), f.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true)) else { return };
    match load_flat(storage, flat, rel, enabled) {
        Ok(()) => log::info!("flat: loaded {rel} (enabled: {enabled})"),
        Err(e) => log::warn!("flat: cannot load {rel}: {e}"),
    }
}

/// Restores the master dark (config key `dark`), same idea as the flat.
fn load_saved_dark(storage: &Storage, flat: &rustflare_capture::SharedFlat) {
    let cfg = crate::config::read(storage.config_file());
    let Some(d) = cfg.get("dark") else { return };
    let Some(rel) = d.get("master").and_then(|v| v.as_str()) else { return };
    match load_dark(storage, flat, rel, d.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true), d.get("exposure_us").and_then(|v| v.as_f64()), d.get("gain").and_then(|v| v.as_f64())) {
        Ok(()) => log::info!("dark: loaded {rel}"),
        Err(e) => log::warn!("dark: cannot load {rel}: {e}"),
    }
}

impl Engine {
    fn run(mut self, rx: Receiver<Request>) {
        // saved settings (site, flat, guiding) are in the status from the first request, not only after the first event
        self.publish();
        loop {
            self.poll_sink();
            self.poll_stack();
            self.auto_connect_tick();
            self.poll_mount();
            self.step_goto();
            self.step_flip();
            self.step_auto_flip();
            self.step_resume();
            self.sync_mount_trace();
            self.flush_controls();
            if self.draining.is_some() && self.last_publish.elapsed() >= Duration::from_millis(200) {
                self.publish();
            }
            loop {
                match rx.try_recv() {
                    Ok(r) => self.dispatch(r),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return self.shutdown(),
                }
            }
            if self.camera.is_none() {
                match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(r) => self.dispatch(r),
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return self.shutdown(),
                }
                continue;
            }
            match self.camera.as_mut().unwrap().capture(Duration::from_secs(10)) {
                Ok(mut frame) => {
                    frame.bayer = self.camera.as_ref().and_then(|c| c.bayer());
                    if let Some(c) = self.status.camera.as_mut() {
                        c.bayer = frame.bayer.map(|b| b.name());
                    }
                    self.on_frame(frame)
                }
                Err(e) => self.camera_failed(e),
            }
        }
    }

    fn shutdown(&mut self) {
        self.abort_job("shutting down");
        self.stop_guiding_quietly();
        self.draining = None; // joins the writer: buffered frames are flushed before exit
        self.mount = None; // drivers stop motion on drop
    }

    fn dispatch(&mut self, r: Request) {
        let res = self.handle(r.cmd);
        self.check_link();
        if res.is_ok() {
            self.status.error = None;
        }
        self.publish();
        let _ = r.reply.send(res);
    }

    /// What the profile says is being imaged: `Some(None)` the Sun, `Some(Some(body))` the Moon or a planet, `None` when nothing is
    /// known (the `other` profile, or the planet profile before any Go to a planet).
    fn target_body(&self) -> Option<Option<rustflare_tracking::bodies::Body>> {
        use rustflare_tracking::bodies::Body;
        match self.status.profile {
            "moon" => Some(Some(Body::Moon)),
            "planet" => self.planet_target.map(Some),
            "other" => None,
            _ => Some(None),
        }
    }

    /// The name a recording gets when none was typed: the target's for the Moon and planets, nothing for the Sun (as it always was).
    fn default_file_name(&self) -> Option<String> {
        match self.status.profile {
            "moon" => Some("Moon".into()),
            "planet" => Some(self.planet_target.map_or("Planet", |b| b.name()).into()),
            _ => None,
        }
    }

    /// The word the engine uses for the thing in the picture where it speaks (errors, notes, the flip's messages).
    fn noun(&self) -> &'static str {
        match self.status.profile { "moon" => "Moon", "planet" => "planet", "other" => "target", _ => "Sun" }
    }

    /// Right ascension (hours) and declination (degrees) of a target now: the Sun, or a body for the saved site.
    fn target_equatorial(&self, t: Option<Option<rustflare_tracking::bodies::Body>>, unix: f64) -> Option<(f64, f64)> {
        match t? {
            None => Some(rustflare_tracking::solar::sun_equatorial(unix)),
            Some(b) => self.site.map(|st| rustflare_tracking::bodies::equatorial(b, unix, st.latitude, st.longitude)),
        }
    }

    /// The radius to expect: measured from the data if the optics' value turned out wrong, else the optics'.
    fn radius_hint(&self) -> Option<f64> {
        self.measured_radius.or(self.expected_radius)
    }

    /// Tracks the radius measured from partial-disc limb fits that disagree with the optics.
    fn learn_radius(&mut self, sun: Option<&SunLocation>) {
        match sun {
            Some(s) if s.partial && s.radius_from_optics == Some(false) => {
                self.radius_samples.push_back(s.radius);
                if self.radius_samples.len() > 30 {
                    self.radius_samples.pop_front();
                }
                if self.radius_samples.len() >= 10 {
                    let mut v: Vec<f64> = self.radius_samples.iter().copied().collect();
                    v.sort_by(|a, b| a.total_cmp(b));
                    self.measured_radius = Some(v[v.len() / 2]);
                }
            }
            Some(s) if s.partial && s.radius_from_optics == Some(true) => {
                // the optics are consistent with the data: nothing to correct
                self.radius_samples.clear();
                self.measured_radius = None;
            }
            _ => {}
        }
    }

    fn refresh_optics(&mut self) {
        use chrono::Datelike;
        use rustflare_tracking::solar;
        let day = chrono::Utc::now().ordinal();
        let (pixel, source) = match (self.optics.pixel_size_um, self.camera_pixel_um) {
            (Some(p), _) => (Some(p), Some("manual")),
            (None, Some(p)) => (Some(p), Some("camera")),
            _ => (None, None),
        };
        let bin = self.optics.binning.max(1);
        let effective = self.optics.focal_length_mm.map(|f| f * self.optics.focal_multiplier);
        let scale = effective.zip(pixel).map(|(f, p)| solar::plate_scale_arcsec_per_px(f, p, bin));
        let now_unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
        // the radius of what the profile says is imaged, in arcseconds: the Sun's of the day, the Moon's from its distance, a planet's globe
        let target_arcsec = match self.target_body() {
            Some(None) => Some(solar::angular_radius_arcsec(day)),
            Some(Some(b)) => Some(rustflare_tracking::bodies::angular_radius_arcsec(b, now_unix)),
            None => None,
        };
        self.expected_radius = effective.zip(pixel).zip(target_arcsec).map(|((f, p), r)| r / solar::plate_scale_arcsec_per_px(f, p, bin));
        self.status.optics = OpticsStatus {
            focal_length_mm: self.optics.focal_length_mm,
            focal_multiplier: self.optics.focal_multiplier,
            effective_focal_mm: effective,
            pixel_size_override_um: self.optics.pixel_size_um,
            pixel_size_um: pixel,
            pixel_size_source: source,
            binning: bin,
            scale_arcsec_per_px: scale,
            solar_radius_arcsec: solar::angular_radius_arcsec(day),
            expected_radius_px: self.expected_radius,
            measured_radius_px: self.measured_radius,
            target_radius_arcsec: target_arcsec,
            suggested_effective_focal_mm: self.measured_radius.zip(pixel).zip(target_arcsec).map(|((r, p), a)| 206.265 * p * bin as f64 * r / a),
            suggested_multiplier: self.measured_radius.zip(pixel).zip(target_arcsec).zip(self.optics.focal_length_mm).map(|(((r, p), a), f)| 206.265 * p * bin as f64 * r / a / f),
            setup: crate::status::SetupStatus { active: self.setups.active.clone(), modified: self.setups.modified(&self.current_use()) },
        };
    }

    /// Is the Pi's clock synchronised to a time server? (`timedatectl`; None where it cannot be read.) Checked at most
    /// once a minute unless forced.
    fn refresh_ntp(&mut self, force: bool) {
        if !force && self.last_ntp_check.elapsed() < Duration::from_secs(60) {
            return;
        }
        self.last_ntp_check = Instant::now();
        self.ntp_sync = std::process::Command::new("timedatectl")
            .args(["show", "-p", "NTPSynchronized", "--value"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| match String::from_utf8_lossy(&o.stdout).trim() {
                "yes" => Some(true),
                "no" => Some(false),
                _ => None,
            });
    }

    /// Follows a GoTo: once the mount has slewed and stopped, switch solar tracking on (the point of "Go to Sun").
    fn step_goto(&mut self) {
        let (started, moved, rate, target) = match self.goto.as_mut() {
            Some(g) if g.last_check.elapsed() >= Duration::from_secs(1) => {
                g.last_check = Instant::now();
                (g.started, g.moved, g.rate, g.target)
            }
            _ => return,
        };
        let Some(m) = self.mount.as_mut() else {
            self.goto = None;
            return;
        };
        let Ok(st) = m.state() else { return };
        let moved = moved || st.slewing == Some(true);
        if let Some(g) = self.goto.as_mut() {
            g.moved = moved;
        }
        if started.elapsed() > Duration::from_secs(300) {
            self.goto = None;
            self.status.error = Some("the GoTo did not finish within 5 minutes".into());
            return;
        }
        if st.slewing == Some(false) && (moved || started.elapsed() > Duration::from_secs(8)) {
            self.goto = None;
            let r = m.set_tracking_rate(rate).and_then(|_| m.set_tracking(true));
            match r {
                Ok(()) => {
                    self.mount_tracking = true;
                    self.mount_rate = rate;
                    log::info!("GoTo {target} finished; tracking is on");
                }
                Err(e) => self.status.error = Some(format!("the mount arrived, but tracking could not be switched on: {e}")),
            }
            self.publish();
        }
    }

    /// Where the mount points and where the Sun is in the picture right now, for the flip to come back to.
    fn snapshot_pre(&mut self, now_unix: f64) -> Result<Value, String> {
        let cam = self.status.camera.as_ref().ok_or("no camera open")?;
        let (w, h) = (cam.width as f64, cam.height as f64);
        let sun = self.status.sun.as_ref().ok_or_else(|| format!("the {} must be in view to record its position", self.noun()))?;
        let (sx, sy, sr) = (sun.pos.x, sun.pos.y, sun.radius);
        let m = self.mount.as_mut().ok_or("no mount connected")?;
        let (ra, dec) = m.equatorial().map_err(s)?.ok_or("this mount does not report where it points, so the position cannot be recorded")?;
        let site = self.site.map(|st| rustflare_tracking::solar::local_sidereal_hours(now_unix, st.longitude));
        // the target's own place in the sky (the keys keep their old names; for the Sun they are the Sun's, as they always were)
        let target = self.target_body();
        let (tra, tdec) = self.target_equatorial(target, now_unix).unwrap_or_else(|| rustflare_tracking::solar::sun_equatorial(now_unix));
        let mut pre = json!({ "unix": now_unix, "ra_hours": ra, "dec_deg": dec, "sun_x": sx, "sun_y": sy, "sun_radius": sr, "frame_w": w, "frame_h": h, "lst_hours": site,
                "sun_ra_hours": tra, "sun_dec_deg": tdec });
        if let Some(Some(b)) = target { pre["target"] = json!(b.id()); }
        Ok(pre)
    }

    /// What is left of the running capture, as a spec that can be started again (cycles or bursts already done are taken off).
    fn remaining_spec(&self) -> Option<ResumeSpec> {
        let Job::Capture(j) = &self.job else { return None };
        let done = j.burst_no.saturating_sub(1);
        match self.last_capture.clone()? {
            ResumeSpec::Bursts { name, frames, seconds, interval_s, bursts, format, on_full, home_when_done } => {
                let bursts = bursts.map(|b| b.saturating_sub(done).max(1));
                Some(ResumeSpec::Bursts { name, frames, seconds, interval_s, bursts, format, on_full, home_when_done })
            }
            ResumeSpec::Lanes { name, kind, cycle_interval_s, lanes, cycles, format, on_full, home_when_done } => {
                let n = lanes.len().max(1) as u32;
                let cycles = cycles.map(|c| c.saturating_sub(done / n).max(1));
                Some(ResumeSpec::Lanes { name, kind, cycle_interval_s, lanes, cycles, format, on_full, home_when_done })
            }
        }
    }

    fn spec_command(spec: ResumeSpec, suffix: &str) -> Command {
        let more = |n: Option<String>| {
            let base = n.unwrap_or_else(|| "seq".into());
            let base = base.trim_end_matches("-after-flip").trim_end_matches("-resumed").to_string();
            Some(format!("{base}{suffix}"))
        };
        match spec {
            ResumeSpec::Bursts { name, frames, seconds, interval_s, bursts, format, on_full, home_when_done } => Command::CaptureBursts { name: more(name), frames, seconds, interval_s, bursts, format, on_full, home_when_done },
            ResumeSpec::Lanes { name, kind, cycle_interval_s, lanes, cycles, format, on_full, home_when_done } => Command::CaptureLanes { name: more(name), kind, cycle_interval_s, lanes, cycles, format, on_full, home_when_done },
        }
    }

    /// Keeps the mount command log pointed at today's storage folder (it can change at run time).
    /// Tells other programs on this machine (the stacker) whether the recorder is busy and where it writes: `capture.state` next to the
    /// settings file, `{"state":"idle","busy":false,"next_in_s":null,"dir":"...","updated":<unix seconds>}`. `busy` is true while
    /// frames are being recorded or written out; a recording that is waiting for its next burst is not busy and says how long the
    /// gap is. It is rewritten on a change and every 10 s: a file that has not been refreshed for a while means the recorder is not
    /// running (so nothing is recording).
    fn write_state_file(&mut self) {
        let c = &self.status.capture;
        let busy = c.state != "idle" && c.next_in_s.is_none();
        let key = format!("{}/{}", c.state, busy);
        if let Some((last, at)) = &self.state_file {
            if *last == key && at.elapsed() < Duration::from_secs(10) {
                return;
            }
        }
        let path = self.storage.config_file().with_file_name("capture.state");
        let updated = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let body = json!({ "state": c.state, "busy": busy, "next_in_s": c.next_in_s, "dir": self.storage.dir().to_string_lossy(), "updated": updated }).to_string();
        self.alog.replace(path, body);
        self.state_file = Some((key, Instant::now()));
    }

    fn sync_mount_trace(&mut self) {
        self.write_state_file();
        let dir = self.storage.internal_log_dir();
        if self.trace_dir.as_ref() != Some(&dir) {
            rustflare_mount::trace::set_dir(Some(dir.clone()));
            self.trace_dir = Some(dir);
        }
        // keep the always-on logs inside their limit: at start and then once an hour
        if self.last_prune.map_or(true, |t| t.elapsed() > Duration::from_secs(3600)) {
            self.last_prune = Some(Instant::now());
            let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
            let n = crate::logs::prune(&self.trace_dir.clone().unwrap_or_default(), crate::logs::MAX_TOTAL_BYTES, crate::logs::MAX_DAYS, &today);
            if n > 0 { log::info!("logs: removed {n} old day folder(s)"); }
        }
    }

    /// Starts the paused recording again once the automatic flip is done.
    fn step_resume(&mut self) {
        let Some(at) = self.resume_at else { return };
        if Instant::now() < at || self.flip.is_some() || self.require_idle().is_err() {
            return;
        }
        self.resume_at = None;
        let Some(spec) = self.resume_after_flip.take() else { return };
        let cmd = Self::spec_command(spec, "-after-flip");
        match self.handle(cmd) {
            Ok(_) => self.event("capture_resumed", json!({ "after": "meridian flip" })),
            Err(e) => self.status.error = Some(format!("could not resume the recording after the flip: {e}")),
        }
        self.publish();
    }

    /// Unattended flip: shortly before the meridian record the pointing and the Sun's place, after it (or as soon as the
    /// mount stops tracking by itself) stop the capture, send the mount to the recorded position and re-centre.
    fn step_auto_flip(&mut self) {
        if !self.auto_flip.enabled || self.last_auto_check.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.last_auto_check = Instant::now();
        let Some(mins) = self.status.flip_target.as_ref().map(|k| k.minutes_to_meridian) else { return };
        if mins > self.auto_flip.record_min + 30.0 {
            self.auto_done = false; // a new crossing is coming
            self.auto_late = false;
            self.auto_pre = None;
            return;
        }
        // Switched on after the moment to flip, with no position recorded before it: the mount cannot be told which side of the pier it is on
        // (the same sky position reads the same either way), so a flip now could turn a mount that is already flipped back again.
        if self.auto_pre.is_none() && mins < -self.auto_flip.flip_min && !self.auto_late && !self.auto_done {
            self.auto_late = true;
            self.event("auto_flip_late", json!({ "minutes_to_meridian": mins, "flip_min": self.auto_flip.flip_min }));
            log::info!("auto flip: switched on {:.0} min after the meridian with nothing recorded: no flip this crossing", -mins);
        }
        if self.auto_late || self.auto_done || self.mount.is_none() || self.flip.is_some() || self.goto.is_some() || mins < -90.0 {
            return;
        }
        let now_unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
        if self.auto_pre.is_none() && mins <= self.auto_flip.record_min && self.mount_tracking && self.status.sun.is_some() {
            match self.snapshot_pre(now_unix) {
                Ok(pre) => {
                    log::info!("auto flip: recorded the position {mins:.1} min before the meridian");
                    self.event("auto_flip_recorded", json!({ "minutes_to_meridian": mins, "pre": pre }));
                    self.auto_pre = Some(pre);
                }
                Err(e) => {
                    // say so (once in a while): this failing silently is what left the first real flip without a recorded position
                    if self.last_auto_warn.map_or(true, |t| t.elapsed() > Duration::from_secs(30)) {
                        self.last_auto_warn = Some(Instant::now());
                        log::warn!("auto flip: cannot record the position: {e}");
                        self.event("auto_flip_record_failed", json!({ "why": e, "minutes_to_meridian": mins }));
                        self.status.error = Some(format!("automatic meridian flip cannot record the position: {e}"));
                    }
                }
            }
        }
        let stopped_by_mount = mins <= 0.0 && !self.mount_tracking && self.auto_pre.is_some();
        if !(mins <= -self.auto_flip.flip_min || stopped_by_mount) {
            return;
        }
        if !matches!(self.job, Job::Idle) {
            // pause the recording: it starts again by itself once the flip is done
            if matches!(self.job, Job::Capture(_)) && self.resume_after_flip.is_none() {
                self.resume_after_flip = self.remaining_spec();
            }
            self.abort_job("paused for the automatic meridian flip");
        }
        if self.require_idle().is_err() {
            return; // still writing the buffered frames: try again next second
        }
        let restore = self.auto_pre.is_some();
        if let Some(pre) = self.auto_pre.clone() {
            self.last_flip = Some(json!({ "pre": pre }));
        }
        match self.handle(Command::StartFlip { record_only: false, restore }) {
            Ok(_) => log::info!("auto flip: started {mins:.1} min past the meridian"),
            Err(e) => self.status.error = Some(format!("automatic meridian flip: {e}")),
        }
        self.auto_done = true;
        self.publish();
    }

    fn cancel_flip(&mut self, why: &str) {
        self.resume_after_flip = None;
        self.resume_at = None;
        if self.flip.take().is_some() {
            self.status.flip = None;
            if let Some(l) = self.last_flip.as_mut() {
                l["result"] = json!(format!("cancelled: {why}"));
            }
        }
    }

    fn finish_flip(&mut self, result: String, post_error_px: Option<f64>) {
        if let Some(f) = self.flip.take() {
            let total = f.started.elapsed().as_secs_f64();
            let rec = json!({ "pre": f.pre, "slew_s": f.slew_s, "total_s": total, "image_turned_over": f.centre.map(|c| c.1), "target_px": f.centre.map(|c| c.0), "post_error_px": post_error_px, "result": result });
            let _ = crate::config::set(self.storage.config_file(), "last_flip", rec.clone());
            self.event("flip_done", rec.clone());
            if f.centre.is_some_and(|c| c.1) { self.sun_model.flipped(); }
            self.last_flip = Some(rec);
        }
        self.status.flip = None;
        if self.resume_after_flip.is_some() {
            if self.last_flip.as_ref().and_then(|l| l.get("result")).and_then(|r| r.as_str()).is_some_and(|r| r.starts_with("done")) {
                self.resume_at = Some(Instant::now() + Duration::from_secs(5)); // let the mount and guiding settle first
            } else {
                self.resume_after_flip = None;
                self.status.error = Some(format!("the meridian flip did not finish cleanly: the recording was NOT resumed, check the {} and restart it", self.noun()));
            }
        }
    }

    /// Follows a flip assist: slew, solar tracking back on, then centre the Sun at its pre-flip place.
    fn step_flip(&mut self) {
        let noun = self.noun();
        let Some(f) = self.flip.as_mut() else { return };
        if f.last_check.elapsed() < Duration::from_secs(1) {
            return;
        }
        f.last_check = Instant::now();
        let elapsed = f.started.elapsed();
        self.status.flip = Some(FlipStatus { phase: f.phase, elapsed_s: elapsed.as_secs_f64(), detail: String::new() });
        if elapsed > Duration::from_secs(1500) {
            self.finish_flip("gave up after 25 minutes".into(), None);
            self.status.error = Some("meridian flip assist timed out".into());
            return;
        }
        match f.phase {
            "slewing" => {
                let Some(m) = self.mount.as_mut() else { return self.finish_flip("the mount disconnected".into(), None) };
                let Ok(st) = m.state() else { return };
                f.moved = f.moved || st.slewing == Some(true);
                if st.slewing == Some(false) && (f.moved || elapsed > Duration::from_secs(8)) {
                    f.slew_s = Some(elapsed.as_secs_f64());
                    f.slew_done_at = Some(Instant::now());
                    let rate = f.rate;
                    let r = m.set_tracking_rate(rate).and_then(|_| m.set_tracking(true));
                    match r {
                        Ok(()) => {
                            self.mount_tracking = true;
                            self.mount_rate = rate;
                        }
                        Err(e) => self.status.error = Some(format!("flip: arrived, but {} tracking could not be switched on: {e}", rate_word(rate))),
                    }
                    f.phase = "settling";
                }
            }
            "settling" => {
                // let the mount and the image settle, then look for the Sun
                if f.slew_done_at.is_some_and(|t| t.elapsed() >= Duration::from_secs(6)) {
                    let (w, h) = (self.status.camera.as_ref().map(|c| c.width as f64).unwrap_or(0.0), self.status.camera.as_ref().map(|c| c.height as f64).unwrap_or(0.0));
                    let g = |k: &str| f.pre.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let before = Vec2::new(g("sun_x"), g("sun_y"));
                    match self.status.sun.as_ref() {
                        None => {
                            if f.slew_done_at.is_some_and(|t| t.elapsed() >= Duration::from_secs(60)) {
                                self.finish_flip(format!("the {} is not in view after the flip: centre it by hand, then press Hold position", noun), None);
                            }
                        }
                        Some(sun) if (w - g("frame_w")).abs() > 1.0 || (h - g("frame_h")).abs() > 1.0 => {
                            let _ = sun;
                            self.finish_flip("the frame size changed since the position was recorded".into(), None);
                        }
                        Some(sun) => {
                            let (target, rotated) = rustflare_tracking::flip::choose_target(before, sun.pos, w, h);
                            let cal = self.calibration.and_then(|c| if rotated { rustflare_tracking::flip::flipped_calibration(&c) } else { Some(c) });
                            f.centre = Some(((target.x, target.y), rotated));
                            match cal {
                                None => self.finish_flip(format!("the mount {} but there is no guiding calibration: centre the {} by hand and calibrate", if rotated { "turned over" } else { "did not turn over" }, noun), None),
                                Some(cal) => {
                                    self.status.guiding.target = Some((target.x, target.y));
                                    self.guide = GuideState::Guiding(Guider::new(target, cal, self.guide_params));
                                    self.drift_model.clear();
                                    self.dead = None;
                                    f.phase = "centring";
                                }
                            }
                        }
                    }
                }
            }
            "recalibrating" => {
                // the flip may have swapped the axes: measure them again, then go back to the target
                match &self.guide {
                    GuideState::Calibrating(_) => {}
                    _ if self.cal_generation != f.cal_gen => {
                        let (Some(cal), Some((t, _))) = (self.calibration, f.centre) else { return self.finish_flip("recalibrated, but the target is unknown".into(), None) };
                        let target = Vec2::new(t.0, t.1);
                        self.status.guiding.target = Some(t);
                        self.guide = GuideState::Guiding(Guider::new(target, cal, self.guide_params));
                        self.drift_model.clear();
                        self.dead = None;
                        f.good_since = None;
                        f.phase = "re-centring";
                    }
                    _ => {
                        self.status.error = Some("flip: recalibration failed, guiding is off: calibrate again".into());
                        self.finish_flip(format!("the {} is back at its place but the recalibration failed, so guiding is off", self.noun()), None);
                    }
                }
            }
            phase => {
                // centring / re-centring: done when the averaged error stays small for a few seconds
                let small = self.status.guiding.error_px.is_some_and(|e| e < (self.guide_params.deadband_px * 2.0).max(4.0));
                if !matches!(self.guide, GuideState::Guiding(_)) {
                    return self.finish_flip("guiding stopped during centring".into(), self.status.guiding.error_px);
                }
                if small {
                    let t = *f.good_since.get_or_insert_with(Instant::now);
                    if t.elapsed() >= Duration::from_secs(4) {
                        let turned = f.centre.map(|c| c.1).unwrap_or(false);
                        let err = self.status.guiding.error_px;
                        if phase == "centring" && f.was_guiding {
                            f.cal_gen = self.cal_generation;
                            f.phase = "recalibrating";
                            self.status.guiding.error_px = None;
                            self.guide = GuideState::Calibrating(Calibrator::new());
                            return;
                        }
                        let tail = if f.was_guiding { ", recalibrated and guiding" } else { " and held" };
                        self.finish_flip(format!("done: the image {}, the {} is back at its place{tail}", if turned { "turned over (rotated 180 degrees)" } else { "did not turn over" }, noun), err);
                    }
                } else {
                    f.good_since = None;
                    if elapsed > Duration::from_secs(900) {
                        let e = self.status.guiding.error_px;
                        self.finish_flip(format!("could not bring the {} back within the time limit: check the calibration", noun), e);
                    }
                }
            }
        }
    }

    fn publish(&mut self) {
        self.refresh_optics();
        self.refresh_ntp(false);
        {
            let s = self.flat.lock().unwrap();
            let cur_exp = self.status.camera.as_ref().and_then(|c| c.controls.iter().find(|c| c.id == CTRL_EXPOSURE_US)).map(|c| c.value);
            let cur_gain = self.status.camera.as_ref().and_then(|c| c.controls.iter().find(|c| c.id == "gain")).map(|c| c.value);
            let close = |a: Option<f64>, b: Option<f64>| match (a, b) { (Some(a), Some(b)) => (a - b).abs() <= 0.01 * a.abs().max(b.abs()).max(1.0), _ => false };
            self.status.darks = DarksStatus { available: s.dark.is_some(), applied: s.dark_enabled && s.dark.is_some(), name: s.dark_name.clone(), exposure_us: s.dark_exposure_us, gain: s.dark_gain, matches_camera: close(s.dark_exposure_us, cur_exp) && close(s.dark_gain, cur_gain) };
            self.status.flats = FlatsStatus { available: s.field.is_some(), applied: s.enabled && s.field.is_some(), name: s.name.clone() };
        }
        let gp = self.guide_params;
        self.status.guide_params = GuideParamsStatus { window_s: gp.window.as_secs_f64(), deadband_px: gp.deadband_px, gain: gp.gain, speed: gp.speed, fast_speed: gp.fast_speed, fast_above_px: gp.fast_above_px };
        {
            let c = self.corona.lock().unwrap();
            self.status.coronagraph = CoronagraphStatus { enabled: c.enabled, scale: c.scale };
            self.status.capture.direct_io.wanted = self.direct_io;
            let col = self.colour.lock().unwrap();
            self.status.colour = ColourStatus { mode: col.mode, r: col.r, b: col.b, applied: col.applied };
        }
        self.status.guiding.held = self.guide_hold.is_some();
        self.status.guiding.dead_reckoning = self.dead.is_some();
        self.status.session_log = SessionLogStatus { lost_frames: self.save_lost, lost_saved: self.lost_saved, enabled: self.slog.enabled(), lines: self.slog.lines, frames: self.slog.frames, bytes: self.slog.bytes, file: self.slog.file_name() };
        self.status.capture.eta_s = self.capture_eta();
        self.status.capture.paused = self.paused.is_some() || self.resume_after_flip.is_some();
        self.status.capture.paused_reason = if self.paused.is_some() { Some("paused by you".into()) } else if self.resume_after_flip.is_some() { Some("paused for the meridian flip".into()) } else { None };
        self.status.last_flip = self.last_flip.clone();
        let flip_stage = if !self.auto_flip.enabled { "off" } else if self.flip.is_some() { "flipping" } else if self.auto_late { "late" } else if self.auto_done { "done" } else if self.auto_pre.is_some() { "recorded" } else { "waiting" };
        let mins = self.status.flip_target.as_ref().map(|k| k.minutes_to_meridian);
        let resume = self.resume_after_flip.is_some();
        let message = match (flip_stage, mins) {
            ("flipping", _) => {
                let (phase, secs) = self.flip.as_ref().map(|f| (f.phase, f.started.elapsed().as_secs())).unwrap_or(("", 0));
                let what = match phase {
                    "slewing" => "the mount is turning over".to_string(),
                    "settling" => format!("settling, looking for the {}", self.noun()),
                    "centring" => format!("centring the {} where it was", self.noun()),
                    "recalibrating" => "recalibrating the guider".to_string(),
                    "re-centring" => format!("bringing the {} back to its place", self.noun()),
                    _ => "working".to_string(),
                };
                format!("Meridian flip, {secs} s: {what}.{}", if resume { " The recording is paused and resumes afterwards." } else { "" })
            }
            ("late", Some(m)) => format!("Switched on {:.0} min after the meridian with no position recorded before it, so there is no flip this time: the mount may already be flipped, and flipping it back would lose the {}. It acts at the next crossing.", -m, self.noun()),
            ("done", _) if resume && self.resume_at.is_some() => "Meridian flip done: resuming the recording in a moment.".to_string(),
            ("recorded", Some(m)) => {
                let when = if m + self.auto_flip.flip_min > 0.5 { format!("in {:.0} min", m + self.auto_flip.flip_min) } else { "any moment".to_string() };
                format!("Meridian flip {when}: position recorded{}.", if resume || matches!(self.job, Job::Capture(_)) { "; the recording pauses and resumes by itself" } else { "" })
            }
            ("waiting", Some(m)) if m <= 15.0 && m > 0.0 => format!("Meridian flip in {m:.0} min (automatic; the position is recorded {:.0} min before).", self.auto_flip.record_min),
            _ => String::new(),
        };
        self.status.auto_flip = AutoFlipStatus {
            enabled: self.auto_flip.enabled, record_min: self.auto_flip.record_min, flip_min: self.auto_flip.flip_min,
            stage: flip_stage, message, resume_pending: resume,
        };
        self.status.clock = ClockStatus { ntp_synchronized: self.ntp_sync };
        self.status.site = self.site.map(|s| SiteStatus { latitude: s.latitude, longitude: s.longitude });
        self.status.sky_sun = self.site.map(|s| {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
            let (ra, dec) = rustflare_tracking::solar::sun_equatorial(now);
            let (alt, az) = rustflare_tracking::solar::alt_az(ra, dec, now, s.latitude, s.longitude);
            let (ha, mins) = rustflare_tracking::flip::meridian_timing(ra, rustflare_tracking::solar::local_sidereal_hours(now, s.longitude));
            SkySun { altitude_deg: alt, azimuth_deg: az, ra_hours: ra, dec_deg: dec, hour_angle_h: ha, minutes_to_meridian: mins }
        });
        let target = self.target_body();
        self.status.target = target.map(|t| match t { None => TargetStatus { id: "sun", name: "Sun" }, Some(b) => TargetStatus { id: b.id(), name: b.name() } });
        self.status.flip_target = self.site.zip(target).map(|(st, t)| {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
            let (ra, dec) = self.target_equatorial(Some(t), now).unwrap_or((0.0, 0.0));
            let lst = rustflare_tracking::solar::local_sidereal_hours(now, st.longitude);
            match t {
                // the Sun: exactly the numbers of status.sky_sun
                None => { let (ha, mins) = rustflare_tracking::flip::meridian_timing(ra, lst); FlipTargetStatus { id: "sun", name: "Sun", ra_hours: ra, dec_deg: dec, hour_angle_h: ha, minutes_to_meridian: mins, rate: TrackingRate::Solar } }
                Some(b) => {
                    let (ha, mins) = rustflare_tracking::flip::meridian_timing_at_rate(ra, lst, rustflare_tracking::bodies::hour_angle_rate(b, now, st.latitude, st.longitude));
                    FlipTargetStatus { id: b.id(), name: b.name(), ra_hours: ra, dec_deg: dec, hour_angle_h: ha, minutes_to_meridian: mins, rate: rate_of_body(Some(b)) }
                }
            }
        });
        self.status.sky_targets = self.site.map(|st| {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
            rustflare_tracking::bodies::Body::ALL.iter().map(|&b| {
                let (ra, dec, alt, az) = rustflare_tracking::bodies::altitude_azimuth(b, now, st.latitude, st.longitude);
                SkyTarget { id: b.id(), name: b.name(), altitude_deg: alt, azimuth_deg: az, ra_hours: ra, dec_deg: dec }
            }).collect()
        }).unwrap_or_default();
        self.status.goto = self.goto.as_ref().map(|g| GotoStatus { target: g.target, phase: if g.moved { "slewing" } else { "starting" } });
        self.status.startup = StartupStatus {
            enabled: self.startup.enabled,
            camera: self.startup.camera.as_ref().map(|(d, id)| format!("{d} {id}")),
            mount: self.startup.mount.as_ref().map(|(d, dev)| format!("{d} {}", dev.clone().unwrap_or_default())),
            retrying: self.auto_until.is_some() && self.startup.enabled && ((self.camera.is_none() && self.startup.camera.is_some()) || (self.mount.is_none() && self.startup.mount.is_some())),
        };
        let (queued, rate) = match (&self.job, &self.draining) {
            (Job::Capture(j), _) => (j.sink.queued(), j.sink.rate_bytes_per_s()),
            (_, Some(d)) => (d.queued(), d.rate_bytes_per_s()),
            _ => (0, 0.0),
        };
        let dropped_now = match (&self.job, &self.draining) {
            (Job::Capture(j), _) => Some(j.sink.dropped()),
            (_, Some(d)) => Some(d.dropped()),
            _ => None,
        };
        let c = &mut self.status.capture;
        if let Some(d) = dropped_now {
            c.dropped = d;
        }
        c.buffer_queued = queued;
        c.buffer_capacity = self.buffer_cap;
        c.buffer_mb = self.buffer_mb;
        c.ram_available_mb = mem_available_mb();
        c.disk_mb_s = if rate > 0.0 { rate / 1e6 } else { self.last_disk_mb_s };
        if matches!(self.job, Job::Idle) && self.draining.is_some() {
            c.state = "flushing";
            c.phase = if queued > 0 { format!("writing {queued} buffered frames to disk") } else { "flushing to the storage medium (safe to unplug when done)".into() };
        }
        self.status.after_burst = AfterBurstStatus {
            mode: self.after.mode.id().into(),
            stack_keep_percent: self.after.stack_keep_percent,
            raw_keep_percent: self.after.raw_keep_percent,
            margin: self.after.margin,
            stacker_connected: self.bridge.connected(),
            stacking: self.stack_jobs.len(),
            stacked: self.stacked_count,
            last: self.last_stack_note.clone(),
            stack_s_per_frame: self.stack_s_per_frame,
        };
        if matches!(self.job, Job::Idle) && self.draining.is_some() && self.close_when_stacked {
            self.status.capture.phase = format!("stacking {} burst(s) from memory", self.stack_jobs.len());
        }
        self.status.seq += 1;
        self.last_publish = Instant::now();
        let ms = self.mount_state;
        self.status.mount = self.mount.as_ref().map(|m| MountStatus { info: m.info(), tracking: self.mount_tracking, slewing: ms.slewing, at_home: ms.at_home, park: ms.park, alt_az: ms.alt_az, link: m.link().map(|l| l.kind), link_up: m.link().map_or(true, |l| l.up), rate: self.mount_rate, slew_speeds: m.slew_speeds(), can_search_home: m.can_search_home() });
        self.status.guiding.state = match self.guide {
            GuideState::Off => "off",
            GuideState::Calibrating(_) => "calibrating",
            GuideState::Guiding(_) => "guiding",
        };
        self.status.guiding.calibration = self.calibration;
        self.status_tx.send_replace(self.status.clone());
    }

    // ---------------------------------------------------------------- commands

    fn handle(&mut self, cmd: Command) -> Reply {
        match cmd {
            Command::ListCameras => {
                let mut list = vec![];
                for d in &self.drivers {
                    match d.enumerate() {
                        Ok(c) => list.extend(c),
                        Err(e) => log::warn!("driver {}: {e}", d.name()),
                    }
                }
                Ok(json!(list))
            }
            Command::OpenCamera { driver, id } => {
                self.require_idle()?;
                self.open_camera(&driver, &id)?;
                if driver != "simulator" {
                    self.startup.camera = Some((driver, id));
                    self.save_startup();
                }
                Ok(json!({}))
            }
            Command::CloseCamera => {
                self.require_idle()?;
                self.close_camera();
                self.startup.camera = None; // closing on purpose: do not reopen it by itself
                self.save_startup();
                Ok(json!({}))
            }
            Command::SetControl { id, value } => {
                let cam = self.camera.as_mut().ok_or("no camera open")?;
                cam.set_control(&id, value).map_err(s)?;
                if id == CTRL_EXPOSURE_US { self.last_exposure_change = Instant::now(); }
                if let Some(c) = self.status.camera.as_mut().and_then(|c| c.controls.iter_mut().find(|c| c.id == id)) {
                    c.value = value.clamp(c.min, c.max);
                }
                if !id.starts_with("sim_") {
                    if let Some(cs) = self.status.camera.as_ref() {
                        let key = format!("{}/{}", cs.driver, cs.id);
                        let entry = self.camera_controls.entry(key).or_insert_with(|| json!({}));
                        if let Some(o) = entry.as_object_mut() {
                            o.insert(id.clone(), json!(value.clamp(cs.controls.iter().find(|c| c.id == id).map_or(f64::MIN, |c| c.min), cs.controls.iter().find(|c| c.id == id).map_or(f64::MAX, |c| c.max))));
                        }
                        self.controls_dirty = Some(Instant::now()); // written to disk a moment later, not per slider tick
                    }
                }
                Ok(json!({}))
            }
            Command::SetCooler { mut req, immediate } => {
                let cam = self.camera.as_mut().ok_or("no camera open")?;
                let cur = cam.cooler().ok_or("this camera has no cooler")?;
                let temp = cam.temperature_c();
                let ramp = req.on == Some(false) && !immediate && crate::cooler::needs_ramp(cur.on, temp);
                if ramp {
                    // warm the sensor first: the target, the fan and the on/off state are the warm-up's to move
                    req = rustflare_core::CoolerRequest { heater: req.heater, ..Default::default() };
                    self.cooler_warmup = Some(Instant::now() - crate::cooler::STEP_EVERY); // the first step is due now
                } else if req.on.is_some() || req.target_c.is_some() {
                    self.cooler_warmup = None; // the user took over
                }
                let cam = self.camera.as_mut().ok_or("no camera open")?;
                cam.set_cooler(&req).map_err(s)?;
                if let Some(c) = self.status.camera.as_mut() { c.temperature_c = temp.or(c.temperature_c); }
                self.refresh_cooler();
                self.step_warmup();
                Ok(json!({ "warming_up": ramp }))
            }
            Command::ConnectMount { driver, device } => {
                self.connect_mount(&driver, device.as_deref())?;
                if driver != "simulator" {
                    self.startup.mount = Some((driver, device));
                    self.save_startup();
                }
                Ok(json!({}))
            }
            Command::DisconnectMount => {
                self.disconnect_mount();
                self.startup.mount = None;
                self.save_startup();
                Ok(json!({}))
            }
            Command::SetStartup { enabled } => {
                self.startup.enabled = enabled;
                self.auto_until = None;
                self.save_startup();
                Ok(json!({}))
            }
            Command::MountTracking { enabled, rate } => {
                if enabled {
                    self.require_not_parked()?;
                }
                // switching tracking on keeps the rate the mount was last given (a Go to Saturn leaves it sidereal), unless one is asked for
                let want = rate.unwrap_or(self.mount_rate);
                let m = self.mount.as_mut().ok_or("no mount connected")?;
                if enabled {
                    m.set_tracking_rate(want).map_err(s)?;
                    self.mount_rate = want;
                }
                m.set_tracking(enabled).map_err(s)?;
                self.mount_tracking = enabled;
                if !enabled {
                    self.stop_guiding_quietly();
                }
                Ok(json!({}))
            }
            Command::MountSlew { axis, speed } => {
                self.manual_slew_until = Instant::now() + Duration::from_secs(if speed != 0.0 { 3600 } else { 3 });
                if speed != 0.0 && self.goto.is_some() {
                    return Err("a GoTo is running: press STOP first".into());
                }
                if speed != 0.0 {
                    self.require_not_parked()?;
                }
                if !matches!(self.guide, GuideState::Off) {
                    return Err("stop guiding before slewing manually".into());
                }
                let m = self.mount.as_mut().ok_or("no mount connected")?;
                if speed == 0.0 {
                    m.slew(axis, SlewSpeed::zero()).map_err(s)?;
                } else {
                    let rate = RadPerSec(speed * SIDEREAL_RATE.0);
                    if !m.slewing_speed_supported(RadPerSec(rate.0.abs())) {
                        return Err(format!("slew speed {}x sidereal not supported by this mount", speed.abs()));
                    }
                    m.slew(axis, SlewSpeed::Specific(rate)).map_err(s)?;
                }
                Ok(json!({}))
            }
            Command::MountStop => {
                self.goto = None; // STOP also cancels a GoTo
                self.cancel_flip("stopped");
                self.stop_guiding_quietly();
                self.mount.as_mut().ok_or("no mount connected")?.stop().map_err(s)?;
                Ok(json!({}))
            }
            Command::MountHome => {
                self.goto = None;
                self.require_idle()?;
                self.require_not_parked()?;
                self.stop_guiding_quietly();
                self.mount.as_mut().ok_or("no mount connected")?.go_home().map_err(s)?;
                self.mount_tracking = false; // the mount stops tracking to go home; the next poll confirms
                self.manual_slew_until = Instant::now() + Duration::from_secs(60); // going home takes about 30 s and the mount reports "not tracking" meanwhile
                Ok(json!({}))
            }
            Command::MountSearchHome => {
                self.goto = None;
                self.require_idle()?;
                self.require_not_parked()?;
                self.stop_guiding_quietly();
                let m = self.mount.as_mut().ok_or("no mount connected")?;
                if !m.can_search_home() {
                    return Err("this mount cannot search for its home position".into());
                }
                m.search_home().map_err(s)?;
                self.mount_tracking = false;
                self.manual_slew_until = Instant::now() + Duration::from_secs(120); // the search moves both axes to their sensors and takes a while
                self.event("search_home", json!({}));
                Ok(json!({}))
            }
            Command::SetFlatApplied { enabled } => {
                {
                    let mut s = self.flat.lock().unwrap();
                    if enabled && s.field.is_none() {
                        return Err("no master flat yet: capture flats first (Camera tab)".into());
                    }
                    s.enabled = enabled;
                }
                self.save_flat_choice();
                Ok(json!({}))
            }
            Command::SetGuideParams { window_s, deadband_px, gain, speed, fast_speed, fast_above_px } => {
                let ok = (2.0..=60.0).contains(&window_s) && (0.0..=30.0).contains(&deadband_px) && (0.05..=1.0).contains(&gain) && (0.1..=0.9).contains(&speed) && (0.1..=0.9).contains(&fast_speed) && (2.0..=200.0).contains(&fast_above_px);
                if !ok {
                    return Err("out of range: window 2-60 s, dead band 0-30 px, gain 0.05-1, speeds 0.1-0.9, fast above 2-200 px".into());
                }
                self.guide_params = GuideParams { window: Duration::from_secs_f64(window_s), deadband_px, gain, speed, fast_speed, fast_above_px, ..self.guide_params };
                crate::config::set(self.storage.config_file(), "guide", json!({ "window_s": window_s, "deadband_px": deadband_px, "gain": gain, "speed": speed, "fast_speed": fast_speed, "fast_above_px": fast_above_px }))?;
                Ok(json!({}))
            }
            Command::SetFocus { enabled, reset } => {
                if enabled {
                    if self.camera.is_none() {
                        return Err("no camera open".into());
                    }
                    if reset || self.focus.is_none() {
                        let now = Instant::now();
                        self.focus = Some(FocusState { started: now, last_scored: now - Duration::from_secs(1), burst_started: now, burst: vec![], live: None, history: vec![], window: None });
                    }
                    self.status.focus = FocusStatus { enabled: true, ..Default::default() };
                    self.publish();
                } else {
                    self.focus = None;
                    self.status.focus = FocusStatus::default();
                    self.publish();
                }
                Ok(json!({}))
            }
            Command::SetProfile { id } => {
                let Some(p) = crate::status::PROFILES.iter().find(|p| **p == id).copied() else { return Err(format!("unknown profile {id} (use {})", crate::status::PROFILES.join(", "))) };
                let sun = |q: &str| q == "sun_white" || q == "sun_ha";
                if p != self.status.profile && !(sun(p) && sun(self.status.profile)) {
                    // another target: the radius learned for the last one does not apply
                    self.radius_samples.clear();
                    self.measured_radius = None;
                    if self.tracker.is_none() {
                        // and so does the model's estimate: it would refuse a target of another size (the gates are fractions of the radius)
                        self.sun_model.reset();
                        self.status.sun = None;
                        self.corona.lock().unwrap().sun = None;
                    }
                }
                if p == "other" && self.tracker.is_none() {
                    // nothing is assumed about the target: the Sun's outline, mask and chips go away with the detector
                    self.sun_model.reset();
                    self.status.sun = None;
                    self.corona.lock().unwrap().sun = None;
                }
                self.status.profile = p;
                self.sun_model.set_gate_floor(model_gate_floor(p));
                self.peak_stretch.store(p == "planet", std::sync::atomic::Ordering::Relaxed);
                crate::config::set(self.storage.config_file(), "profile", json!(p))?;
                self.event("profile", json!({ "profile": p }));
                Ok(json!({}))
            }
            Command::SetColour { mode, r, b } => {
                let mode: &'static str = match mode.as_str() { "off" => "off", "auto" => "auto", "manual" => "manual", o => return Err(format!("unknown white balance mode {o} (use off, auto or manual)")) };
                if !(0.25..=4.0).contains(&r) || !(0.25..=4.0).contains(&b) {
                    return Err("gains must be between 0.25 and 4".into());
                }
                {
                    let mut c = self.colour.lock().unwrap();
                    c.mode = mode;
                    c.r = r;
                    c.b = b;
                    c.applied = [1.0; 3];
                }
                crate::config::set(self.storage.config_file(), "colour", json!({ "mode": mode, "r": r, "b": b }))?;
                Ok(json!({}))
            }
            Command::SetCoronagraph { enabled, scale } => {
                if !(0.8..=1.2).contains(&scale) {
                    return Err("mask scale must be 0.8 to 1.2 of the Sun's radius".into());
                }
                {
                    let mut c = self.corona.lock().unwrap();
                    c.enabled = enabled;
                    c.scale = scale;
                }
                crate::config::set(self.storage.config_file(), "coronagraph", json!({ "enabled": enabled, "scale": scale }))?;
                Ok(json!({}))
            }
            Command::SetSessionLog { enabled, frame_every_s } => {
                if enabled {
                    let dir = self.storage.dir().join("logs");
                    self.slog.start(&dir, frame_every_s)?;
                } else {
                    self.slog.stop();
                }
                Ok(json!({}))
            }
            Command::DebugState => Ok(self.debug_state()),
            Command::DebugSnapshot => {
                let frame = self.last_frame.clone().ok_or("no frame received yet")?;
                let path = self.save_frame_note(&frame, "debug", "snapshot requested").ok_or("could not write the snapshot (storage?)")?;
                Ok(json!({ "png": format!("/files/{path}.png"), "note": format!("/files/{path}.json") }))
            }
            Command::SetLostFrames { enabled } => {
                self.save_lost = enabled;
                crate::config::set(self.storage.config_file(), "debug", json!({ "lost_frames": enabled }))?;
                Ok(json!({}))
            }
            Command::SetRoi { fraction } => {
                self.require_idle()?;
                self.require_not_flats()?;
                if !(0.1..=1.0).contains(&fraction) {
                    return Err("region must be 10% to 100% of the sensor".into());
                }
                let cam = self.camera.as_mut().ok_or("no camera open")?;
                let sensor = cam.sensor_size().ok_or("this camera cannot report its sensor size, so a region of interest is not available")?;
                let roi = rustflare_core::centred_roi(sensor, fraction);
                cam.set_roi(roi).map_err(s)?;
                // The frame geometry changed: positions, flats and darks no longer line up, so tracking starts over.
                self.stop_guiding_quietly();
                self.tracker = None;
                self.status.tracking = None;
                self.status.sun = None;
                self.last_frame = None;
                self.refresh_roi();
                Ok(json!({ "width": roi.width, "height": roi.height, "x": roi.x, "y": roi.y }))
            }
            Command::StartFlip { record_only, restore } => {
                self.require_idle()?;
                self.require_not_parked()?;
                if self.flip.is_some() || self.goto.is_some() {
                    return Err("a GoTo or flip is already running: press STOP first".into());
                }
                let now_unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
                let pre = if restore {
                    self.last_flip.as_ref().and_then(|v| v.get("pre")).cloned().ok_or("no recorded position: record one before the flip")?
                } else {
                    self.snapshot_pre(now_unix)?
                };
                if record_only {
                    self.last_flip = Some(json!({ "pre": pre }));
                    let _ = crate::config::set(self.storage.config_file(), "last_flip", self.last_flip.clone().unwrap_or_default());
                    return Ok(json!({ "recorded": true }));
                }
                let f = |k: &str| pre.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0);
                // the position was recorded for a target (the Sun unless it says otherwise): carry the pointing by that target's own motion
                let pre_body = pre.get("target").and_then(|v| v.as_str()).and_then(rustflare_tracking::bodies::Body::from_id);
                let rate = rate_of_body(pre_body);
                let target_now = self.target_equatorial(Some(pre_body), now_unix).unwrap_or_else(|| rustflare_tracking::solar::sun_equatorial(now_unix));
                let target = rustflare_tracking::flip::carried_target((f("ra_hours"), f("dec_deg")), (f("sun_ra_hours"), f("sun_dec_deg")), target_now);
                let was_guiding = matches!(self.guide, GuideState::Guiding(_));
                self.stop_guiding_quietly();
                if let Some(st) = self.site {
                    let st = SiteTime { latitude_deg: st.latitude, longitude_east_deg: st.longitude, unix_secs: now_unix as i64, utc_offset_minutes: chrono::Local::now().offset().local_minus_utc() / 60 };
                    let _ = self.mount.as_mut().ok_or("no mount connected")?.set_site_time(&st);
                }
                self.mount.as_mut().ok_or("no mount connected")?.goto_equatorial(target.0, target.1).map_err(s)?;
                self.mount_tracking = false;
                self.last_flip = Some(json!({ "pre": pre, "started_unix": now_unix }));
                self.flip = Some(FlipState { started: Instant::now(), pre, moved: false, last_check: Instant::now(), slew_done_at: None, slew_s: None, phase: "slewing", centre: None, good_since: None, was_guiding, cal_gen: 0, rate });
                Ok(json!({ "target_ra_hours": target.0, "target_dec_deg": target.1 }))
            }
            Command::SetAutoFlip { enabled, record_min, flip_min } => {
                if !(1.0..=60.0).contains(&record_min) || !(0.0..=60.0).contains(&flip_min) {
                    return Err("record 1 to 60 minutes before the meridian and flip 0 to 60 minutes after it".into());
                }
                self.auto_flip = AutoFlip { enabled, record_min, flip_min };
                if !enabled { self.auto_pre = None; }
                self.auto_done = false;
                self.auto_late = false;
                crate::config::set(self.storage.config_file(), "auto_flip", json!(self.auto_flip))?;
                Ok(json!({}))
            }
            Command::SetSite { latitude, longitude } => {
                if !(-90.0..=90.0).contains(&latitude) || !(-180.0..=180.0).contains(&longitude) {
                    return Err("latitude must be -90..90 and longitude -180..180 (east positive)".into());
                }
                self.site = Some(Site { latitude, longitude });
                crate::config::set(self.storage.config_file(), "site", json!({ "latitude": latitude, "longitude": longitude }))?;
                Ok(json!({}))
            }
            Command::MountGoto { target, allow_unsynced_clock, allow_below_horizon } => {
                self.require_idle()?;
                self.require_not_parked()?;
                if self.mount.is_none() {
                    return Err("no mount connected".into());
                }
                let body = if target == "sun" { None } else { Some(rustflare_tracking::bodies::Body::from_id(&target).ok_or(format!("unknown target {target}"))?) };
                let (name, id, rate) = match body {
                    None => ("the Sun", "sun", TrackingRate::Solar),
                    Some(b) => (b.name(), b.id(), if b == rustflare_tracking::bodies::Body::Moon { TrackingRate::Lunar } else { TrackingRate::Sidereal }),
                };
                let site = self.site.ok_or("set your site first: More > Site (latitude and longitude)")?;
                self.refresh_ntp(true);
                if self.ntp_sync == Some(false) && !allow_unsynced_clock {
                    return Err(format!("the Pi's clock is not synchronised to a time server, so the position of {name} and the time given to the mount may be off: check the clock, or go anyway"));
                }
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
                let (ra, dec) = match body {
                    None => rustflare_tracking::solar::sun_equatorial(now),
                    Some(b) => rustflare_tracking::bodies::equatorial(b, now, site.latitude, site.longitude),
                };
                let (alt, _az) = rustflare_tracking::solar::alt_az(ra, dec, now, site.latitude, site.longitude);
                if alt < -1.0 && (body.is_none() || !allow_below_horizon) {
                    return Err(format!("{name} is below the horizon (altitude {alt:.0} degrees)"));
                }
                self.stop_guiding_quietly();
                let st = SiteTime { latitude_deg: site.latitude, longitude_east_deg: site.longitude, unix_secs: now as i64, utc_offset_minutes: chrono::Local::now().offset().local_minus_utc() / 60 };
                let m = self.mount.as_mut().ok_or("no mount connected")?;
                m.set_site_time(&st).map_err(s)?;
                m.goto_equatorial(ra, dec).map_err(s)?;
                self.mount_tracking = false; // the mount stops for the slew; tracking is switched on at arrival
                if let Some(b) = body.filter(|b| *b != rustflare_tracking::bodies::Body::Moon) {
                    self.planet_target = Some(b); // the planet profile's flip follows this one
                    let _ = crate::config::set(self.storage.config_file(), "planet_target", json!(b.id()));
                }
                self.goto = Some(GotoState { started: Instant::now(), moved: false, last_check: Instant::now(), target: id, rate });
                self.event("goto", json!({ "target": id, "ra_hours": ra, "dec_deg": dec, "altitude_deg": alt }));
                Ok(json!({ "target": id, "ra_hours": ra, "dec_deg": dec, "altitude_deg": alt }))
            }
            Command::StartTracking { mode, x, y, radius } => {
                self.require_not_flats()?;
                let f = self.last_frame.as_ref().ok_or("no frame received yet")?;
                let t = match mode.as_str() {
                    "sun_disc" => Tracker::sun_disc(f),
                    "limb" => Tracker::limb(f, radius.or(self.radius_hint())),
                    "planet" => Tracker::planet(f),
                    "anchor" => Tracker::anchor(f, Vec2::new(x.ok_or("anchor needs x,y")?, y.ok_or("anchor needs x,y")?)),
                    other => return Err(format!("unknown tracking mode {other}")),
                }
                .map_err(s)?;
                self.stop_guiding_quietly();
                let p = t.position();
                self.tracker = Some(t);
                self.sun_model.reset(); // the tracker's circle starts the model afresh
                self.lost = false;
                Ok(json!({}))
            }
            Command::StopTracking => {
                self.stop_guiding_quietly();
                self.tracker = None;
                self.status.tracking = None;
                Ok(json!({}))
            }
            Command::Calibrate => {
                self.require_ready_to_guide(false)?;
                self.guide = GuideState::Calibrating(Calibrator::new());
                Ok(json!({}))
            }
            Command::StartGuiding { center } => {
                self.require_ready_to_guide(true)?;
                let pos = if center {
                    let f = self.last_frame.as_ref().ok_or("no frame received yet")?;
                    Vec2::new(f.width as f64 / 2.0, f.height as f64 / 2.0)
                } else {
                    self.tracker.as_ref().unwrap().position().pos
                };
                self.status.guiding.target = Some((pos.x, pos.y));
                self.event("guiding_start", json!({ "target": [pos.x, pos.y], "centred": center }));
                self.guide = GuideState::Guiding(Guider::new(pos, self.calibration.unwrap(), self.guide_params));
                self.drift_model.clear();
                self.dead = None;
                Ok(json!({}))
            }
            Command::StopGuiding => {
                self.stop_guiding_quietly();
                Ok(json!({}))
            }
            Command::CaptureDarks { name, frames } => {
                self.require_capture_ready()?;
                if !(1..=500).contains(&frames) {
                    return Err("dark frame count must be 1 to 500".into());
                }
                let cam = self.status.camera.as_ref().ok_or("no camera open")?;
                if cam.width == 0 {
                    return Err("no frame received yet".into());
                }
                let fmt = if cam.bits > 8 { rustflare_core::PixelFormat::Mono16 } else { rustflare_core::PixelFormat::Mono8 };
                let gain = cam.controls.iter().find(|c| c.id == "gain").map(|c| c.value).unwrap_or(0.0);
                let (w, h) = (cam.width, cam.height);
                let exposure_us = self.exposure();
                self.stop_guiding_quietly();
                self.job = Job::Darks(Box::new(Darks { stem: self.new_stem("dark", name.as_deref()), frames, acc: FlatAccumulator::new(w, h, fmt), exposure_us, gain, skip: 2, raw: None }));
                self.set_capture("flats", "dark frames: keep the cap on", Some(frames));
                Ok(json!({}))
            }
            Command::SetDarkApplied { enabled } => {
                {
                    let mut s = self.flat.lock().unwrap();
                    if enabled && s.dark.is_none() {
                        return Err("no master dark yet: capture darks first (Camera tab)".into());
                    }
                    s.dark_enabled = enabled;
                }
                self.save_flat_choice();
                Ok(json!({}))
            }
            Command::CaptureProminence { name, factor } => {
                self.require_capture_ready()?;
                if !(2.0..=64.0).contains(&factor) {
                    return Err("exposure factor must be 2 to 64".into());
                }
                if self.status.sun.is_none() {
                    return Err("no Sun found in the frame: the disc position is needed for the composite".into());
                }
                let exp = self.exposure();
                self.job = Job::Prominence(Box::new(Prominence { stem: self.new_stem("prom", name.as_deref()), factor, original_exposure: exp, long_exposure: exp, disc: None, skip: 0 }));
                self.set_capture("single", "disc exposure", None);
                Ok(json!({}))
            }
            Command::CaptureSingle { name, format } => {
                self.require_capture_ready()?;
                let fits = match format.as_deref() {
                    None | Some("png") => false,
                    Some("fits") => true,
                    Some(o) => return Err(format!("unknown format {o} (use png or fits)")),
                };
                self.job = Job::Single { stem: self.new_stem("frame", name.as_deref()), fits };
                self.set_capture("single", "waiting for frame", None);
                Ok(json!({}))
            }
            Command::CaptureBursts { name, frames, seconds, interval_s, bursts, format, on_full, home_when_done } => {
                let rs = ResumeSpec::Bursts { name: name.clone(), frames, seconds, interval_s, bursts, format: format.clone(), on_full: on_full.clone(), home_when_done };
                self.require_capture_ready()?;
                self.require_after_burst_ready()?;
                if frames.is_none() && seconds.is_none() {
                    return Err("give a frame count and/or a duration per burst".into());
                }
                if frames == Some(0) || seconds.is_some_and(|s| s <= 0.0) {
                    return Err("frame count and duration must be positive".into());
                }
                let interval = match interval_s {
                    Some(i) if i >= 0.5 => Some(Duration::from_secs_f64(i)),
                    Some(_) => return Err("interval must be at least 0.5 s".into()),
                    None => None,
                };
                let total_bursts = if interval.is_some() { bursts } else { Some(1) };
                if total_bursts == Some(0) {
                    return Err("number of bursts must be positive".into());
                }
                let format = match format.as_deref() {
                    None | Some("ser") => Format::Ser,
                    Some("png") => Format::Png,
                    Some("fits") => Format::Fits,
                    Some(o) => return Err(format!("unknown format {o} (use ser, png or fits)")),
                };
                let drop_when_full = match on_full.as_deref() {
                    None | Some("wait") => false,
                    Some("drop") => true,
                    Some(o) => return Err(format!("unknown buffer policy {o} (use wait or drop)")),
                };
                let cam = self.status.camera.as_ref().ok_or("no camera open")?;
                if cam.width == 0 {
                    return Err("no frame received yet".into());
                }
                let frame_bytes = cam.width as u64 * cam.height as u64 * (cam.bits as u64 / 8).max(1);
                let cap = self.buffer_capacity(frame_bytes);
                self.buffer_cap = cap;
                let kind = if interval.is_some() { "burst" } else { "seq" };
                self.status.capture.stopped_early = None;
                self.status.capture.frames_lost = None;
                self.event("capture_start", json!({ "kind": "sequence" }));
                self.last_capture = Some(rs);
                self.job = Job::Capture(Box::new(CaptureJob {
                    lanes: None,
                    name: name.filter(|n| !n.trim().is_empty()).or_else(|| self.default_file_name()).unwrap_or_default(),
                    kind,
                    format,
                    burst_frames: frames,
                    burst_seconds: seconds,
                    interval,
                    total_bursts,
                    burst_no: 0,
                    phase: CapPhase::Waiting { next_at: Instant::now() },
                    max_file: self.storage.info().max_file_bytes,
                    last_check: Instant::now(),
                    sink: FrameSink::with_policy(cap, drop_when_full),
                    drop_when_full,
                    sink_no: self.next_sink_no(),
                    cur: None,
                    warned_no_stacker: false,
                    disc_track: Vec::new(),
                    home_when_done,
                    bursts_done: 0,
                    burst_time_sum: 0.0,
                }));
                self.status.capture.dropped = 0;
                self.set_capture(if interval.is_some() { "bursts" } else { "sequence" }, "starting", frames);
                self.status.capture.bursts_total = total_bursts;
                Ok(json!({ "buffer_frames": cap, "fits_in_buffer": frames.map_or(true, |f| f as usize <= cap) }))
            }
            Command::CaptureLanes { name, kind, cycle_interval_s, lanes, cycles, format, on_full, home_when_done } => {
                let rs = ResumeSpec::Lanes { name: name.clone(), kind: kind.clone(), cycle_interval_s, lanes: lanes.clone(), cycles, format: format.clone(), on_full: on_full.clone(), home_when_done };
                if cycle_interval_s.is_some_and(|i| i < 1.0) {
                    return Err("the time between cycles must be at least 1 s".into());
                }
                let kind_label: &'static str = match kind.as_deref() {
                    None | Some("hdr") => "hdr",
                    Some("prominence") => "prominence",
                    Some(o) => return Err(format!("unknown lane kind {o} (use hdr or prominence)")),
                };
                self.require_capture_ready()?;
                self.require_after_burst_ready()?;
                if !(2..=6).contains(&lanes.len()) {
                    return Err("give 2 to 6 lanes".into());
                }
                let mut seen = std::collections::HashSet::new();
                for l in &lanes {
                    if sanitize(&l.name).is_empty() || !seen.insert(sanitize(&l.name)) {
                        return Err("every lane needs its own name".into());
                    }
                    if l.exposure_us <= 0.0 || (l.frames.is_none() && l.seconds.is_none()) || l.frames == Some(0) || l.seconds.is_some_and(|s| s <= 0.0) {
                        return Err(format!("lane {}: give a positive exposure and a frame count and/or duration", l.name));
                    }
                }
                if cycles == Some(0) {
                    return Err("number of cycles must be positive".into());
                }
                let format = match format.as_deref() {
                    None | Some("ser") => Format::Ser,
                    Some("png") => Format::Png,
                    Some("fits") => Format::Fits,
                    Some(o) => return Err(format!("unknown format {o} (use ser, png or fits)")),
                };
                let drop_when_full = match on_full.as_deref() {
                    None | Some("wait") => false,
                    Some("drop") => true,
                    Some(o) => return Err(format!("unknown buffer policy {o} (use wait or drop)")),
                };
                let cam = self.status.camera.as_ref().ok_or("no camera open")?;
                if cam.width == 0 {
                    return Err("no frame received yet".into());
                }
                let frame_bytes = cam.width as u64 * cam.height as u64 * (cam.bits as u64 / 8).max(1);
                let cap = self.buffer_capacity(frame_bytes);
                self.buffer_cap = cap;
                let gain = cam.controls.iter().find(|c| c.id == "gain").map(|c| c.value);
                let original = (self.exposure(), gain);
                let dir = self.new_stem(kind_label, name.as_deref());
                let _ = std::fs::create_dir_all(&dir);
                let manifest = self.sidecar("lane-session", json!({ "kind": kind_label, "cycles": cycles, "lanes": lanes.iter().map(|l| json!({ "name": l.name, "exposure_us": l.exposure_us, "gain": l.gain, "frames": l.frames, "seconds": l.seconds, "mask_disc": l.mask_disc })).collect::<Vec<_>>() }));
                let _ = std::fs::write(dir.join("session.json"), serde_json::to_vec_pretty(&manifest).unwrap_or_default());
                let total = cycles.map(|c| c * lanes.len() as u32);
                let n_lanes = lanes.len();
                self.status.capture.stopped_early = None;
                self.status.capture.frames_lost = None;
                self.event("capture_start", json!({ "kind": "lanes" }));
                self.last_capture = Some(rs);
                self.job = Job::Capture(Box::new(CaptureJob {
                    lanes: Some(Lanes { specs: lanes, dir, original, cycle_interval: cycle_interval_s.map(Duration::from_secs_f64), cycle_started: Instant::now(), current: 0, skip: 0 }),
                    name: name.filter(|n| !n.trim().is_empty()).or_else(|| self.default_file_name()).unwrap_or_default(),
                    kind: kind_label,
                    format,
                    burst_frames: None,
                    burst_seconds: None,
                    interval: Some(Duration::ZERO),
                    total_bursts: total,
                    burst_no: 0,
                    phase: CapPhase::Waiting { next_at: Instant::now() },
                    max_file: self.storage.info().max_file_bytes,
                    last_check: Instant::now(),
                    sink: FrameSink::with_policy(cap, drop_when_full),
                    drop_when_full,
                    sink_no: self.next_sink_no(),
                    cur: None,
                    warned_no_stacker: false,
                    disc_track: Vec::new(),
                    home_when_done,
                    bursts_done: 0,
                    burst_time_sum: 0.0,
                }));
                self.status.capture.dropped = 0;
                self.set_capture("bursts", "starting", None);
                self.status.capture.bursts_total = total;
                Ok(json!({ "lanes": n_lanes, "buffer_frames": cap }))
            }
            Command::MountQuery { what } => {
                let m = self.mount.as_mut().ok_or("no mount connected")?;
                let reply = m.query(&what).map_err(s)?;
                Ok(json!({ "what": what, "reply": reply }))
            }
            Command::SetDirectIo { enabled } => {
                if enabled {
                    let dir = self.storage.dir();
                    rustflare_capture::direct_io_supported(&dir).map_err(|e| format!("this drive does not allow direct writes: {e}"))?;
                }
                self.direct_io = enabled;
                crate::config::set(self.storage.config_file(), "direct_io", json!(enabled))?;
                self.status.capture.direct_io = self.direct_io_state(None);
                Ok(json!({ "direct_io": self.status.capture.direct_io }))
            }
            Command::SetAfterBurst { patch } => {
                let mut a = self.after.clone();
                if let Some(m) = patch["mode"].as_str() {
                    a.mode = AfterMode::from_id(m).ok_or("mode must be video, video_stack, stack_only or best_frames")?;
                }
                for (key, field, lo, hi) in [("stack_keep_percent", &mut a.stack_keep_percent, 1.0, 100.0), ("raw_keep_percent", &mut a.raw_keep_percent, 1.0, 100.0), ("margin", &mut a.margin, 1.0, 5.0)] {
                    if let Some(x) = patch[key].as_f64() {
                        if !(lo..=hi).contains(&x) {
                            return Err(format!("{key} must be between {lo} and {hi}"));
                        }
                        *field = x;
                    }
                }
                if matches!(a.mode, AfterMode::StackOnly | AfterMode::BestFrames) && !self.bridge.connected() {
                    return Err(NO_STACKER.into());
                }
                crate::config::set(self.storage.config_file(), "after_burst", a.to_json())?;
                self.after = a;
                self.event("after_burst_set", self.after.to_json());
                Ok(self.after.to_json())
            }
            Command::AckNotices { id } => {
                for n in self.status.notices.iter_mut().filter(|n| id.map_or(true, |i| i == n.id)) {
                    n.acked = true;
                }
                Ok(json!({}))
            }
            Command::SetBuffer { mb } => {
                if !(64..=8192).contains(&mb) {
                    return Err("buffer must be between 64 and 8192 MB".into());
                }
                self.buffer_mb = mb;
                crate::config::set(self.storage.config_file(), "capture", json!({ "buffer_mb": mb }))?;
                Ok(json!({}))
            }
            Command::CaptureFlats { name, frames, level } => {
                self.require_capture_ready()?;
                if frames == 0 {
                    return Err("frame count must be positive".into());
                }
                let ctrl = self.status.camera.as_ref().and_then(|c| c.controls.iter().find(|c| c.id == CTRL_EXPOSURE_US)).ok_or("camera has no exposure control")?;
                let target = FlatTarget {
                    level: level.unwrap_or(0.5).clamp(0.1, 0.9),
                    min_exposure_us: ctrl.min,
                    max_exposure_us: ctrl.max,
                    ..Default::default()
                };
                let exp = ctrl.value;
                // Flats are taken with a diffuser / defocused: sun tracking is meaningless.
                self.stop_guiding_quietly();
                self.tracker = None;
                self.status.tracking = None;
                let stem = self.new_stem("flat", name.as_deref());
                self.job = Job::Flats(Box::new(Flats { stem, target, frames, phase: FlatPhase::Adjust { iter: 0, skip: 1 }, original_exposure: exp, exposure_used: exp }));
                self.set_capture("flats", "finding exposure", Some(frames));
                Ok(json!({}))
            }
            Command::SetOptics { focal_length_mm, focal_multiplier, pixel_size_um, binning } => {
                self.set_optics(OpticsSettings { focal_length_mm, focal_multiplier, pixel_size_um, binning })?;
                Ok(json!({}))
            }
            Command::ListSetups => Ok(self.setups.to_json(&self.current_use())),
            Command::SaveSetup { name, body } => {
                let cur = self.current_use();
                let mut next = self.setups.clone();
                let saved = next.save(&name, &body, &cur)?;
                next.persist(self.storage.config_file())?;
                self.setups = next;
                self.event("optics_setup_saved", json!({ "name": saved.name }));
                Ok(json!({ "saved": saved, "active": self.setups.active, "modified": self.setups.modified(&cur) }))
            }
            Command::DeleteSetup { name } => {
                let mut next = self.setups.clone();
                let gone = next.delete(&name)?;
                next.persist(self.storage.config_file())?;
                self.setups = next;
                self.event("optics_setup_deleted", json!({ "name": gone.name }));
                Ok(json!({ "deleted": gone.name, "active": self.setups.active }))
            }
            Command::ApplySetup { name } => self.apply_setup(&name),
            Command::CapturePause => {
                if !matches!(self.job, Job::Capture(_)) {
                    return Err("only a running sequence or lane recording can be paused".into());
                }
                let spec = self.remaining_spec().ok_or("nothing to resume from this recording")?;
                self.paused = Some(spec);
                self.abort_job("paused by user");
                Ok(json!({}))
            }
            Command::CaptureResume => {
                let spec = self.paused.take().ok_or("nothing is paused")?;
                let cmd = Self::spec_command(spec.clone(), "-resumed");
                match self.handle(cmd) {
                    Ok(v) => { self.event("capture_resumed", json!({ "after": "pause" })); Ok(v) }
                    Err(e) => { self.paused = Some(spec); Err(e) }
                }
            }
            Command::CaptureStop => {
                self.paused = None;
                self.resume_after_flip = None;
                self.resume_at = None;
                if matches!(self.job, Job::Idle) {
                    return Err("nothing is being captured".into());
                }
                self.last_capture = None;
                self.resume_after_flip = None;
                self.abort_job("stopped by user");
                Ok(json!({}))
            }
        }
    }

    fn require_not_parked(&self) -> Result<(), String> {
        match self.mount_state.park {
            Some(ParkState::Parked) | Some(ParkState::Parking) => Err("the mount is parked: unpark it first".into()),
            _ => Ok(()),
        }
    }

    /// Reads the mount's real state every couple of seconds: it can be changed from outside (its own park
    /// button, another app), so the app must not assume it is still doing what we last told it.
    fn poll_mount(&mut self) {
        self.check_link();
        if self.mount.is_none() || self.last_mount_poll.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.last_mount_poll = Instant::now();
        match self.mount.as_mut().map(|m| m.state()) {
            Some(Ok(st)) => {
                self.mount_poll_failures = 0;
                if let Some(t) = st.tracking {
                    // A move we made ourselves (the slew pad, a Go to, a flip, going home, guiding pulses, calibration) makes the AM3 report
                    // "not tracking" for as long as it lasts, and it tracks again by itself afterwards. That is not "the mount stopped":
                    // follow it silently. A stop with no move of ours behind it is reported after a few seconds, with the mount's reason.
                    let now = Instant::now();
                    let moving = now < self.manual_slew_until || self.goto.is_some() || self.flip.is_some() || self.pulsing || now < self.pulse_settle_until
                        || matches!(self.guide, GuideState::Calibrating(_));
                    if t == self.mount_tracking || t || moving {
                        if t != self.mount_tracking {
                            log::info!("mount tracking is now {}{}", if t { "on" } else { "off" }, if moving { " (it is moving)" } else { " (changed outside RustFlare)" });
                        }
                        self.mount_tracking = t;
                        self.tracking_off_since = None;
                        self.tracking_off_noticed = false;
                    } else {
                        let since = *self.tracking_off_since.get_or_insert(now);
                        if now.duration_since(since) >= Duration::from_secs(6) && !self.tracking_off_noticed {
                            self.tracking_off_noticed = true;
                            let why = self.mount.as_mut().and_then(|m| m.why_not_tracking());
                            if why.as_deref() == Some("it is tracking") {
                                self.tracking_off_since = None; // the flag was a blip: the mount says it tracks
                            } else {
                                log::info!("mount tracking is now off (changed outside RustFlare): {}", why.as_deref().unwrap_or("no reason given"));
                                self.event("mount_stopped_tracking", json!({ "why": why }));
                                if !matches!(self.guide, GuideState::Off) {
                                    self.stop_guiding_quietly();
                                    self.status.error = Some("the mount stopped tracking (changed on the mount itself): guiding stopped".into());
                                }
                                self.notice("warning", format!("The mount stopped tracking by itself: {}.", why.unwrap_or_else(|| "it gave no reason".into())));
                                self.mount_tracking = false;
                            }
                        }
                    }
                }
                let changed = st != self.mount_state;
                self.mount_state = st;
                if changed {
                    self.publish(); // the state can change under us; show it now, not at the next command
                }
            }
            Some(Err(e)) => {
                self.mount_poll_failures += 1;
                if self.mount_poll_failures == 3 {
                    // A dead link (e.g. the mount re-enumerated on USB and our handle is stale): drop it and keep
                    // retrying the remembered device, instead of showing a mount that no longer answers.
                    log::warn!("lost contact with the mount: {e}");
                    self.stop_guiding_quietly();
                    self.mount = None;
                    self.mount_tracking = false;
                    self.mount_state = MountState::default();
                    self.mount_poll_failures = 0;
                    self.status.error = Some(format!("lost contact with the mount ({e}); reconnecting"));
                    if self.startup.enabled && self.startup.mount.is_some() {
                        self.auto_until = Some(Instant::now() + Duration::from_secs(600));
                    }
                }
            }
            None => {}
        }
    }

    /// A Bluetooth link that dropped: stop guiding, drop the driver (which sends a stop where the link still allows
    /// it) and let the auto-reconnect retry. The mount is never left believed-connected while the link is gone.
    fn check_link(&mut self) {
        let lost = self.mount.as_ref().and_then(|m| m.link()).is_some_and(|l| l.kind == "ble" && !l.up);
        if !lost {
            return;
        }
        log::warn!("Bluetooth link to the mount lost");
        self.stop_guiding_quietly();
        if let Some(m) = self.mount.as_mut() {
            let _ = m.stop();
        }
        self.disconnect_mount();
        self.status.error = Some("the Bluetooth link to the mount was lost; the mount may still be moving until its own limits: check it. Reconnecting".into());
        if self.startup.enabled && self.startup.mount.is_some() {
            self.auto_until = Some(Instant::now() + Duration::from_secs(600));
            self.last_auto_try = Instant::now() - Duration::from_secs(5);
        }
        self.publish();
    }

    fn require_idle(&self) -> Result<(), String> {
        if !matches!(self.job, Job::Idle) {
            Err("a capture is running".into())
        } else if let Some(d) = &self.draining {
            Err(format!("still writing {} buffered frames to disk", d.queued()))
        } else {
            Ok(())
        }
    }

    /// RAM buffer size in frames: the configured budget, capped to 70 % of the memory that is actually free.
    fn buffer_capacity(&self, frame_bytes: u64) -> usize {
        let mut budget = self.buffer_mb * 1024 * 1024;
        if let Some(avail) = mem_available_mb() {
            budget = budget.min(avail * 1024 * 1024 * 7 / 10);
        }
        (budget / frame_bytes.max(1)).clamp(4, 200_000) as usize
    }
    /// Re-reads the region of interest from the camera into the status.
    fn refresh_roi(&mut self) {
        self.status.roi = self.camera.as_ref().and_then(|c| {
            let (r, (sw, sh)) = (c.roi()?, c.sensor_size()?);
            Some(RoiStatus { x: r.x, y: r.y, width: r.width, height: r.height, sensor_width: sw, sensor_height: sh })
        });
    }

    fn require_not_flats(&self) -> Result<(), String> {
        if matches!(self.job, Job::Flats(_)) { Err("not while capturing flats".into()) } else { Ok(()) }
    }
    fn data_dir(&self) -> PathBuf {
        self.storage.dir()
    }
    fn require_capture_ready(&mut self) -> Result<(), String> {
        if self.camera.is_none() {
            return Err("no camera open".into());
        }
        self.require_idle()?;
        let dir = self.storage.ensure_ready()?;
        // direct writes, if wanted and if this drive allows them; if not, the normal way, said out loud
        let problem = if self.direct_io { rustflare_capture::direct_io_supported(&dir).err() } else { None };
        rustflare_capture::set_direct_io(self.direct_io && problem.is_none());
        if let Some(p) = &problem {
            self.event("direct_io_unavailable", json!({ "why": p }));
            self.notice("warning", format!("Direct writes are switched on but this drive does not allow them ({p}): recording the normal way, through the page cache."));
        }
        self.status.capture.direct_io = self.direct_io_state(problem);
        Ok(())
    }
    fn direct_io_state(&self, problem: Option<String>) -> DirectIoStatus {
        DirectIoStatus { wanted: self.direct_io, active: self.direct_io && problem.is_none(), problem }
    }
    fn require_ready_to_guide(&self, need_cal: bool) -> Result<(), String> {
        self.require_not_parked()?;
        if self.tracker.is_none() {
            return Err("start tracking first".into());
        }
        if self.lost {
            return Err("target is lost".into());
        }
        if self.mount.is_none() {
            return Err("no mount connected".into());
        }
        if !self.mount_tracking {
            return Err("enable the mount's sky tracking first".into());
        }
        if need_cal && self.calibration.is_none() {
            return Err("calibrate first".into());
        }
        if !matches!(self.guide, GuideState::Off) {
            return Err("already calibrating/guiding".into());
        }
        Ok(())
    }

    fn open_camera(&mut self, driver: &str, id: &str) -> Result<(), String> {
        self.close_camera();
        let d = self.drivers.iter().find(|d| d.name() == driver).ok_or(format!("unknown driver {driver}"))?;
        let mut cam = d.open(id).map_err(s)?;
        let info = cam.info();
        let mut controls = cam.controls().map_err(s)?;
        // Re-apply what was last set on this camera (exposure, gain, ...), so a reboot comes back as it was.
        if let Some(saved) = self.camera_controls.get(&format!("{}/{}", info.driver, info.id)).and_then(|v| v.as_object()) {
            let mut ids: Vec<&String> = saved.keys().collect();
            ids.sort_by_key(|k| k.as_str() != "high_bit_depth"); // the output format first: it can change the others' limits
            for id in ids {
                if let (Some(v), Some(c)) = (saved[id].as_f64(), controls.iter_mut().find(|c| &c.id == id)) {
                    let v = v.clamp(c.min, c.max);
                    if cam.set_control(id, v).is_ok() {
                        c.value = v;
                    }
                }
            }
        }
        self.camera_pixel_um = cam.pixel_size_um();
        let bayer = self.camera.as_ref().and_then(|c| c.bayer()).map(|b| b.name());
        self.status.camera = Some(CameraStatus { driver: info.driver, id: info.id, name: info.name, controls, bayer, ..Default::default() });
        self.camera = Some(cam);
        self.cooler_warmup = None;
        self.refresh_roi();
        self.refresh_cooler();
        Ok(())
    }

    /// Reads the cooler into the status (an uncooled camera gets `available: false`).
    fn refresh_cooler(&mut self) {
        let st = self.camera.as_mut().and_then(|c| c.cooler());
        let warming = self.cooler_warmup.is_some();
        let Some(c) = self.status.camera.as_mut() else { return };
        c.cooler = match st {
            Some(s) => CoolerView {
                available: true, on: s.on, target_c: s.target_c, target_min_c: s.target_min_c, target_max_c: s.target_max_c,
                power_pct: s.power_pct, temp_c: c.temperature_c, fan: s.fan, heater: s.heater, warming_up: warming && s.on,
            },
            None => CoolerView::default(),
        };
    }

    /// One move of the gentle switch-off (`cooler.rs`), when one is due.
    fn step_warmup(&mut self) {
        use crate::cooler::*;
        let Some(last) = self.cooler_warmup else { return };
        let Some(v) = self.status.camera.as_ref().map(|c| c.cooler.clone()) else { return };
        if !v.on {
            self.cooler_warmup = None; // already off (by hand or by the camera)
            self.refresh_cooler();
            return;
        }
        if last.elapsed() < STEP_EVERY { return }
        let (Some(temp), Some(target)) = (v.temp_c, v.target_c) else {
            // no way to tell: a plain switch-off
            self.finish_warmup();
            return;
        };
        match warmup_step(temp, target, v.power_pct, v.target_max_c.unwrap_or(30.0)) {
            Warmup::Finish => self.finish_warmup(),
            Warmup::Raise(t) => {
                let r = rustflare_core::CoolerRequest { target_c: Some(t), ..Default::default() };
                if let Some(cam) = self.camera.as_mut() {
                    if let Err(e) = cam.set_cooler(&r) { log::warn!("cooler warm-up: {e}"); }
                }
                self.cooler_warmup = Some(Instant::now());
                self.refresh_cooler();
            }
            Warmup::Wait => {}
        }
    }

    fn finish_warmup(&mut self) {
        let r = rustflare_core::CoolerRequest { on: Some(false), ..Default::default() };
        if let Some(cam) = self.camera.as_mut() {
            if let Err(e) = cam.set_cooler(&r) { log::warn!("cooler switch-off: {e}"); }
        }
        self.cooler_warmup = None;
        self.refresh_cooler();
    }

    fn connect_mount(&mut self, driver: &str, device: Option<&str>) -> Result<(), String> {
        self.disconnect_mount();
        let opts = rustflare_mount::ConnectOptions { ble_token: crate::config::read(self.storage.config_file()).get("zwo_ble_token").and_then(|v| v.as_str()).map(String::from) };
        let m = rustflare_mount::connect_with(driver, device, &self.sky, &opts).map_err(s)?;
        self.adopt_mount(m);
        Ok(())
    }

    fn adopt_mount(&mut self, m: Box<dyn Mount>) {
        self.mount = Some(m);
        // Connecting never changes the mount's tracking; read back what it is actually doing.
        self.mount_tracking = false;
        self.mount_state = MountState::default();
        if let Some(Ok(st)) = self.mount.as_mut().map(|m| m.state()) {
            self.mount_tracking = st.tracking.unwrap_or(false);
            self.mount_state = st;
        }
    }

    /// Writes changed camera control values to the settings file once they have settled for a moment.
    fn flush_controls(&mut self) {
        if self.controls_dirty.is_some_and(|t| t.elapsed() >= Duration::from_millis(1500)) {
            self.controls_dirty = None;
            let _ = crate::config::set(self.storage.config_file(), "camera_controls", Value::Object(self.camera_controls.clone()));
        }
    }

    fn save_startup(&self) {
        let _ = crate::config::set(self.storage.config_file(), "startup", serde_json::to_value(&self.startup).unwrap_or_default());
    }

    /// Reconnects the remembered camera / mount (they may take a while to appear after boot or a USB hiccup).
    /// Never switches on mount tracking.
    fn auto_connect_tick(&mut self) {
        let Some(until) = self.auto_until else { return };
        if !self.startup.enabled || Instant::now() > until {
            self.auto_until = None;
            return;
        }
        if self.last_auto_try.elapsed() < Duration::from_secs(5) {
            return;
        }
        self.last_auto_try = Instant::now();
        let mut changed = false;
        if self.camera.is_none() {
            if let Some((driver, id)) = self.startup.camera.clone() {
                match self.open_camera(&driver, &id) {
                    Ok(()) => { log::info!("reconnected camera {driver}/{id}"); changed = true; }
                    Err(e) => log::debug!("auto camera {driver}/{id}: {e}"),
                }
            }
        }
        if let Some(rx) = &self.pending_mount {
            match rx.try_recv() {
                Ok(Ok(m)) => {
                    self.pending_mount = None;
                    if self.mount.is_none() && self.startup.mount.is_some() {
                        self.adopt_mount(m);
                        log::info!("reconnected mount over Bluetooth (tracking stays off)");
                        changed = true;
                    }
                }
                Ok(Err(e)) => { self.pending_mount = None; log::debug!("auto mount (Bluetooth): {e}"); }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => self.pending_mount = None,
            }
        }
        if self.mount.is_none() && self.pending_mount.is_none() {
            if let Some((driver, device)) = self.startup.mount.clone() {
                if device.as_deref().is_some_and(|d| d.starts_with(rustflare_mount::BLE_PREFIX)) {
                    // Bluetooth may take many seconds (the mount might not be advertising): never on the engine thread.
                    let (tx, rx) = std::sync::mpsc::channel();
                    let (sky, dev) = (self.sky.clone(), device.clone());
                    let opts = rustflare_mount::ConnectOptions { ble_token: crate::config::read(self.storage.config_file()).get("zwo_ble_token").and_then(|v| v.as_str()).map(String::from) };
                    let spawned = std::thread::Builder::new().name("ble-connect".into()).spawn(move || {
                        let _ = tx.send(rustflare_mount::connect_with(&driver, dev.as_deref(), &sky, &opts).map_err(|e| e.to_string()));
                    });
                    if spawned.is_ok() { self.pending_mount = Some(rx); }
                } else {
                match self.connect_mount(&driver, device.as_deref()) {
                    Ok(()) => { log::info!("reconnected mount {driver} {device:?} (tracking stays off)"); changed = true; }
                    Err(e) => log::debug!("auto mount {driver}: {e}"),
                }
                }
            }
        }
        let done = (self.camera.is_some() || self.startup.camera.is_none()) && (self.mount.is_some() || self.startup.mount.is_none()) && self.pending_mount.is_none();
        if done {
            self.auto_until = None;
        }
        if changed {
            self.publish();
        }
    }

    fn close_camera(&mut self) {
        self.cooler_warmup = None;
        self.sun_model.reset();
        self.camera = None;
        self.focus = None;
        self.status.focus = FocusStatus::default();
        self.status.roi = None;
        self.camera_pixel_um = None;
        self.status.camera = None;
        self.last_frame = None;
        self.tracker = None;
        self.status.tracking = None;
        self.stop_guiding_quietly();
    }

    fn disconnect_mount(&mut self) {
        self.pending_mount = None;
        self.stop_guiding_quietly();
        self.mount = None;
        self.mount_tracking = false;
        self.mount_state = MountState::default();
        self.calibration = None;
    }

    fn stop_guiding_quietly(&mut self) {
        self.guide_hold = None;
        self.dead = None;
        let was_active = !matches!(self.guide, GuideState::Off);
        if was_active { self.event("guiding_stop", json!({})); }
        self.guide = GuideState::Off;
        self.status.guiding.target = None;
        self.status.guiding.error_px = None;
        if was_active {
            if let Some(m) = self.mount.as_mut() {
                let _ = m.guide(RadPerSec(0.0), RadPerSec(0.0));
            }
        }
    }

    fn camera_failed(&mut self, e: Error) {
        log::error!("camera failed: {e}");
        self.abort_job("camera error");
        self.close_camera();
        if self.startup.enabled && self.startup.camera.is_some() {
            self.auto_until = Some(Instant::now() + Duration::from_secs(60));
        }
        self.status.error = Some(format!("camera error: {e}"));
        self.publish();
    }

    // ----------------------------------------------------------------- frames

    /// Focus assist: score about 25 frames a second, summarise each burst of a few seconds as a lucky score with its error, and
    /// publish the history and the peak of its smoothed curve.
    fn step_focus(&mut self, frame: &Frame) {
        let noun = self.noun();
        let Some(f) = self.focus.as_mut() else { return };
        let now = Instant::now();
        if now.duration_since(f.last_scored) < Duration::from_millis(40) {
            return;
        }
        f.last_scored = now;
        let sun = self.status.sun.as_ref().map(|s| (s.pos.x, s.pos.y, s.radius));
        // the window stays where the Sun was when the session began; it is placed again once, if the Sun was not known then
        if f.window.map_or(true, |(_, had_sun)| !had_sun && sun.is_some()) {
            f.window = Some((rustflare_tracking::focus::Window::new(sun, frame.width, frame.height), sun.is_some()));
            f.burst.clear();
        }
        let win = f.window.unwrap().0;
        let mut note = None;
        match rustflare_tracking::focus::sharpness(frame, win, sun) {
            Some(sc) => {
                f.burst.push(sc);
                f.live = Some(f.live.map_or(sc, |l| 0.7 * l + 0.3 * sc));
            }
            None if sun.is_some() => note = Some(format!("The {noun} has moved out of the scoring window (a Go to, or a long drift): press Reset history.")),
            None => note = Some(format!("Nothing bright to judge: is the {noun} in view and the cap off?")),
        }
        if sun.is_none() && note.is_none() {
            note = Some(format!("The {noun} is not detected, so the score includes its edge and moves with the {noun}."));
        }
        if now.duration_since(f.burst_started) >= FOCUS_BURST {
            if let Some((l, e)) = rustflare_tracking::focus::lucky_score(&f.burst) {
                let t = now.duration_since(f.started).as_secs_f64();
                // the error of one burst leaves out the seeing between bursts, which is several times larger: show the larger of the two
                let mut scores: Vec<f64> = f.history.iter().map(|h| h[1]).collect();
                scores.push(l);
                f.history.push([t, l, e.max(rustflare_tracking::focus::between_burst_noise(&scores))]);
                if f.history.len() > 400 {
                    f.history.remove(0);
                }
            }
            f.burst.clear();
            f.burst_started = now;
        }
        let last = f.history.last().copied();
        let peak = rustflare_tracking::focus::peak(&f.history);
        let t_now = now.duration_since(f.started).as_secs_f64();
        let skip = f.history.len().saturating_sub(90);
        self.status.focus = FocusStatus {
            enabled: true,
            live: f.live,
            lucky: last.map(|h| h[1]),
            lucky_err: last.map(|h| h[2]),
            best: peak.map(|p| p.1),
            best_err: peak.map(|p| p.2),
            best_age_s: peak.map(|p| t_now - f.history[p.0][0]),
            history: f.history[skip..].to_vec(),
            note,
        };
    }

    fn on_frame(&mut self, frame: Frame) {
        let t = Instant::now();
        self.on_frame_inner(frame);
        self.last_frame_work = t.elapsed();
    }

    fn on_frame_inner(&mut self, frame: Frame) {
        self.step_focus(&frame);
        let now = Instant::now();
        let t_frame = now;
        let ema = |old: f64, new: f64| if old == 0.0 { new } else { 0.95 * old + 0.05 * new };
        if let Some(prev) = self.last_frame_at.replace(now) {
            let gap = now.duration_since(prev).as_secs_f64();
            // a stall: far longer than the frames usually take. Logged with what the machine was doing, so a stall can be traced to
            // a cause (memory, the disk writer, another program) afterwards; at most once a second
            if self.fps_ema > 1.0 && gap > (8.0 / self.fps_ema).max(0.25) && self.last_gap_event.map_or(true, |t| t.elapsed() > Duration::from_secs(1)) {
                self.last_gap_event = Some(now);
                let load1 = std::fs::read_to_string("/proc/loadavg").ok().and_then(|t| t.split_whitespace().next().and_then(|v| v.parse::<f64>().ok()));
                let c = &self.status.capture;
                self.event("frame_gap", json!({ "gap_s": (gap * 1000.0).round() / 1000.0, "usual_s": (1000.0 / self.fps_ema).round() / 1000.0, "capture": c.state, "burst": c.burst,
                    "prev_frame_work_ms": (self.last_frame_work.as_secs_f64() * 1000.0).round(), "waiting_for_camera_ms": ((gap - self.last_frame_work.as_secs_f64()).max(0.0) * 1000.0).round(),
                    "buffer_queued": c.buffer_queued, "disk_mb_s": c.disk_mb_s, "ram_available_mb": mem_available_mb(), "memory": memory_state(), "load1": load1, "log_lines_dropped": self.alog.dropped() }));
            }
            let inst = 1.0 / gap.max(1e-3);
            self.fps_ema = if self.fps_ema == 0.0 { inst } else { 0.9 * self.fps_ema + 0.1 * inst };
        }
        // Analysis (statistics, histogram, Sun location, tracking) runs at ~10 Hz regardless of the camera's
        // frame rate: it is far more than guiding needs, and it keeps fast cameras free for recording.
        let mut mean = self.last_mean;
        if now.duration_since(self.last_analysis) >= Duration::from_millis(100) {
            self.last_analysis = now;
            let t_an = Instant::now();
            let t_st = Instant::now();
            let fs = frame.stats(64);
            self.status.perf.stats_ms = ema(self.status.perf.stats_ms, t_st.elapsed().as_secs_f64() * 1000.0);
            mean = fs.mean;
            self.last_mean = mean;
            let refresh_temp = self.last_temp.elapsed() > Duration::from_secs(3);
            if refresh_temp {
                self.last_temp = now;
            }
            let temp = if refresh_temp { self.camera.as_mut().and_then(|c| c.temperature_c()) } else { None };
            if let Some(c) = self.status.camera.as_mut() {
                c.width = frame.width;
                c.height = frame.height;
                c.bits = (frame.format.bytes_per_pixel() * 8) as u32;
                c.mean = mean;
                c.saturated = fs.channels.as_ref().map_or(fs.saturated, |ch| ch.saturated.iter().cloned().fold(fs.saturated, f64::max));
                c.peak = fs.peak;
                c.histogram = fs.histogram;
                c.histogram_rgb = fs.channels.as_ref().map(|ch| ch.histogram.clone());
                c.saturated_rgb = fs.channels.as_ref().map(|ch| ch.saturated);
                if refresh_temp {
                    c.temperature_c = temp;
                }
            }
            if refresh_temp {
                self.refresh_cooler();
                self.step_warmup();
            }
            // In a lane recording only the reference lane (the shortest exposure that is not coronagraph-masked) shows the
            // disc properly. The long prominence frames are saturated: measuring the Sun on them gives a wrong circle, which
            // then drives the mask and a false "tracking lost". Other lanes keep the last measurement and are not analysed.
            let sparse = matches!(&self.job, Job::Capture(j) if j.lanes.is_some());
            let drift = self.drift_model.drift();
            if let GuideState::Guiding(g) = &mut self.guide { g.set_sparse(sparse); g.set_drift(drift); }
            let analyse = match &self.job {
                Job::Capture(j) => j.lanes.as_ref().map_or(true, |l| l.skip == 0 && l.current == reference_lane(&l.specs)),
                _ => true,
            };
            if !analyse {
                self.status.perf.analysis_ms = ema(self.status.perf.analysis_ms, t_an.elapsed().as_secs_f64() * 1000.0);
            }
            // The tracker measures the Sun on every analysis once it runs; the detector is then only needed when the tracker fails (it is
            // asked for then, see `step_tracking`) and now and then to keep the radius estimate current. Running both every time
            // doubled the work of the engine thread.
            let tracker_on = self.tracker.as_ref().is_some_and(|t| t.mode() != "anchor");
            self.analysis_n = self.analysis_n.wrapping_add(1);
            let need_detect = analyse && self.status.profile != "other" && (!tracker_on || self.analysis_n % 20 == 0);
            let hint = self.radius_hint();
            let t_loc = Instant::now();
            let sun = if need_detect { detect_target(self.status.profile, &frame, hint, if self.sun_held || self.analysis_n % 10 == 0 { None } else { self.status.sun.as_ref() }) } else { None };
            if need_detect {
                self.status.perf.locate_ms = ema(self.status.perf.locate_ms, t_loc.elapsed().as_secs_f64() * 1000.0);
            }
            if analyse {
                if need_detect {
                    self.learn_radius(sun.as_ref());
                }
                // ONE model of where the Sun is: the tracker's measurement feeds it while tracking (known radius, robust fit),
                // the detector otherwise. The Sun outline, the tracking circle, the coronagraph mask and the guider all use it.
                if !tracker_on {
                    self.feed_sun_model(sun, now, (frame.width, frame.height));
                }
                // darkness: much less light than the last minute or so, with the exposure unchanged
                let settled = self.last_exposure_change.elapsed() > Duration::from_secs(20);
                let dark = picture_is_dark(settled, self.bright_ref, mean);
                if settled && !dark {
                    self.bright_ref = if self.bright_ref == 0.0 { mean } else { 0.998 * self.bright_ref + 0.002 * mean }; // about a minute
                }
                let t_tr = Instant::now();
                self.step_tracking(&frame, now, if tracker_on { None } else { sun }, hint, dark);
                self.status.perf.track_ms = ema(self.status.perf.track_ms, t_tr.elapsed().as_secs_f64() * 1000.0);
                self.was_analysing = true;
                self.lane_prev = None;
            } else if sparse && matches!(&self.job, Job::Capture(j) if j.lanes.as_ref().is_some_and(|l| l.skip == 0)) && self.sun_model.current().is_some() {
                // A long-exposure lane frame: its disc is saturated, so its position is biased, but it moves with the Sun. Compare it
                // with the previous long-exposure frame and move the Sun model by the difference, so the outline and the coronagraph
                // mask stay on the image between the disc bursts (the scope can slide hundreds of pixels meanwhile).
                if let Some(m) = locate_sun(&frame, self.radius_hint()) {
                    if let Some((t0, p0)) = self.lane_prev {
                        let d = m.pos - p0;
                        if now.duration_since(t0) < Duration::from_millis(1500) && d.len() < 80.0 {
                            self.sun_model.shift(d);
                            if let Some(cur) = self.sun_model.current() {
                                self.corona.lock().unwrap().sun = Some((cur.pos.x, cur.pos.y, cur.radius));
                                self.status.sun = Some(cur);
                            }
                        }
                    }
                    self.lane_prev = Some((now, m.pos));
                } else {
                    self.lane_prev = None;
                }
                if matches!(self.guide, GuideState::Guiding(_)) {
                    if self.was_analysing { self.step_guide(now, GuideInput::Judge); } else { self.step_guide(now, GuideInput::Tick); }
                }
                self.was_analysing = false;
            } else if matches!(self.guide, GuideState::Guiding(_)) {
                // Lane recordings measure only the reference lane, in bursts with long gaps. Pulses must still end on time, and
                // the corrections are judged when a burst of measurements ends (their frames then carry the long exposures).
                if self.was_analysing { self.step_guide(now, GuideInput::Judge); } else { self.step_guide(now, GuideInput::Tick); }
                self.was_analysing = false;
            } else {
                self.was_analysing = false;
            }
            self.log_session(&frame, now);
            let t_cl = Instant::now();
            if self.last_frame.is_none() || self.last_frame_saved.elapsed() >= Duration::from_millis(500) {
                self.last_frame = Some(frame.clone());
                self.last_frame_saved = Instant::now();
            }
            self.status.perf.clone_ms = ema(self.status.perf.clone_ms, t_cl.elapsed().as_secs_f64() * 1000.0);
            self.status.perf.analysis_ms = ema(self.status.perf.analysis_ms, t_an.elapsed().as_secs_f64() * 1000.0);
        }
        if let Some(c) = self.status.camera.as_mut() {
            c.fps = self.fps_ema;
        }
        if self.last_preview.elapsed() >= Duration::from_millis(66) && self.demand.wanted() {
            self.last_preview = now;
            let _ = self.preview_tx.try_send(frame.clone());
        }
        // The frame is moved (not copied) into the recording path.
        let t_job = Instant::now();
        self.step_job(frame, mean);
        self.status.perf.job_ms = ema(self.status.perf.job_ms, t_job.elapsed().as_secs_f64() * 1000.0);
        if self.last_publish.elapsed() >= Duration::from_millis(150) {
            let t_pub = Instant::now();
            self.publish();
            self.status.perf.publish_ms = ema(self.status.perf.publish_ms, t_pub.elapsed().as_secs_f64() * 1000.0);
        }
        self.status.perf.frame_ms = ema(self.status.perf.frame_ms, t_frame.elapsed().as_secs_f64() * 1000.0);
    }

    /// Feeds the one Sun model with a measurement (or `None`) and publishes its estimate. A cloud, a shadow or an eclipse makes
    /// a measurement jump or vanish, and nothing downstream may follow that: it gets the model's prediction, corrected only by
    /// measurements that agree with it.
    fn feed_sun_model(&mut self, meas: Option<SunLocation>, now: Instant, dims: (u32, u32)) -> Option<crate::sunmodel::Estimate> {
        // the image moves on purpose while slewing, calibrating or pulsing: follow the measurements then
        let free = self.mount_state.slewing == Some(true) || self.goto.is_some() || self.flip.is_some() || now < self.manual_slew_until
            || matches!(self.guide, GuideState::Calibrating(_)) || self.pulsing || now < self.pulse_settle_until;
        // learn what good light looks like, and do not measure in the dark
        let level = self.status.camera.as_ref().map_or(0.0, |c| c.mean);
        if !self.sun_held && meas.is_some() && level > 0.05 {
            self.good_level = Some(self.good_level.map_or(level, |g| 0.995 * g + 0.005 * level));
        }
        let dark = crate::sunmodel::too_dark(self.good_level, level);
        let meas = if dark { None } else { meas };
        let est = self.sun_model.update(meas, now, dims, free, None);
        let was_held = self.sun_held;
        self.sun_held = est.is_some_and(|e| e.predicted && e.since_measured_s > 1.0);
        if self.sun_held != was_held && self.last_sun_event.elapsed() > Duration::from_secs(5) {
            self.last_sun_event = Instant::now();
            self.event(if self.sun_held { "sun_predicted" } else { "sun_measured_again" }, json!({ "estimate": est.map(|e| json!({ "x": e.sun.pos.x, "y": e.sun.pos.y, "r": e.sun.radius, "since_measured_s": e.since_measured_s })), "measurement": meas.map(|f| json!({ "x": f.pos.x, "y": f.pos.y, "r": f.radius })) }));
        }
        let sun = est.map(|e| e.sun);
        self.corona.lock().unwrap().sun = sun.as_ref().map(|s| (s.pos.x, s.pos.y, s.radius));
        self.status.sun = sun;
        est
    }

    fn step_tracking(&mut self, frame: &Frame, now: Instant, detector: Option<SunLocation>, hint: Option<f64>, dark: bool) {
        let profile = self.status.profile;
        let prev = if self.sun_held || self.analysis_n % 10 == 0 { None } else { self.status.sun }; // now and then look in the whole frame
        let Some(t) = self.tracker.as_mut() else { return };
        // frames still in flight after an exposure change (or a lane switch) are over- or under-exposed: not a real loss
        let settling = self.last_exposure_change.elapsed() < Duration::from_secs(3);
        let res = if dark { Err(TrackError::Dark) } else { t.update(frame) };
        let mode = t.mode();
        let anchor = mode == "anchor";
        let dims = (frame.width, frame.height);
        match res {
            Err(_) if settling && !self.lost => {
                if !anchor && !dark { let d = detector.or_else(|| detect_target(profile, frame, hint, prev.as_ref())); self.feed_sun_model(d, now, dims); }
            }
            Ok(p) => {
                self.fail_since = None;
                self.fail_saved = false;
                // the tracker's circle IS the model's circle: one estimate, shown once
                let est = if anchor { None } else {
                    let meas = if p.held { None } else { Some(SunLocation { pos: p.pos, radius: p.radius.unwrap_or(1000.0), partial: true, radius_from_optics: None, fit_rms: None }) };
                    self.feed_sun_model(meas, now, dims)
                };
                let (sx, sy, sr) = est.map(|e| (e.sun.pos.x, e.sun.pos.y, Some(e.sun.radius))).unwrap_or((p.pos.x, p.pos.y, p.radius));
                self.status.tracking = Some(TrackingStatus { mode, x: sx, y: sy, radius: sr, lost: false });
                let was_lost = self.lost;
                self.lost = false;
                if was_lost { self.event("tracking_found", json!({ "x": sx, "y": sy, "radius": sr, "resuming_guiding": self.guide_hold.is_some(), "dead_reckoned_s": self.dead.as_ref().map(|d| d.age(now).as_secs_f64()) })); }
                if was_lost && matches!(self.guide, GuideState::Off) && self.mount_tracking {
                    if let Some((target, cal)) = self.guide_hold.take() {
                        log::info!("tracking found again: guiding resumed");
                        self.event("guiding_resumed", json!({ "target": [target.x, target.y] }));
                        self.status.guiding.target = Some((target.x, target.y));
                        self.guide = GuideState::Guiding(Guider::new(target, cal, self.guide_params));
                        self.drift_model.clear();
                        self.dead = None;
                        self.status.error = None;
                    }
                }
                // What the guider sees must be a trustworthy position. When it is not, and the drift is known, the guider is
                // fed a dead-reckoned one so the framing holds.
                let trusted = if anchor { Some(p.pos) } else { est.filter(|e| !e.predicted).map(|e| e.sun.pos) };
                match trusted {
                    Some(gp) => {
                        if self.dead.take().is_some() {
                            if let GuideState::Guiding(g) = &mut self.guide { g.reset_samples(); }
                            self.event("dead_reckoning_end", json!({}));
                            self.status.error = None;
                        }
                        let applied = if let GuideState::Guiding(g) = &self.guide { g.applied() } else { Vec2::default() };
                        if matches!(self.guide, GuideState::Guiding(_)) { self.drift_model.observe(now, gp, applied); }
                        self.last_good = Some((now, gp, applied));
                        self.step_guide(now, GuideInput::Pos(gp));
                    }
                    None => self.dead_reckon(now),
                }
            }
            Err(e) => {
                if !anchor && !dark {
                    let d = detector.or_else(|| detect_target(profile, frame, hint, prev.as_ref()));
                    if let Some(est) = self.feed_sun_model(d, now, dims) {
                        if let Some(ts) = self.status.tracking.as_mut() { ts.x = est.sun.pos.x; ts.y = est.sun.pos.y; ts.radius = Some(est.sun.radius); }
                    }
                }
                let since = *self.fail_since.get_or_insert(now);
                if !self.fail_saved {
                    self.fail_saved = true;
                    self.save_lost_frame(frame, &e.to_string()); // keep the first bad frame even if it turns out to be a blink
                }
                if !self.lost && now.duration_since(since) < Duration::from_secs(2) {
                    return;
                }
                let first = !self.lost;
                self.lost = true;
                if let Some(ts) = self.status.tracking.as_mut() {
                    ts.lost = true;
                }
                if first {
                    let held = match (&self.guide, self.calibration) { (GuideState::Guiding(g), Some(c)) => Some((g.target, c)), _ => None };
                    let drift = self.drift_model.drift();
                    log::warn!("tracking lost: {e}");
                    self.event("tracking_lost", json!({ "why": e.to_string(), "was_guiding": held.is_some(), "drift_px_s": drift.map(|d| [d.x, d.y]), "seconds_since_exposure_change": self.last_exposure_change.elapsed().as_secs_f64() }));
                    match (&self.guide, drift, self.last_good) {
                        (GuideState::Guiding(_), Some(d), Some((t0, p0, a0))) => {
                            // keep guiding on the learned drift: the framing is what matters
                            self.dead = Some(DeadReckoning::new(t0, p0, a0, d));
                            self.event("dead_reckoning_start", json!({ "drift_px_s": [d.x, d.y] }));
                            self.status.error = Some(format!("tracking lost: {e}. Guiding goes on from the measured drift"));
                        }
                        // a calibration rides out blinks of the tracker (it works from medians); only a long loss ends it
                        (GuideState::Calibrating(_), _, _) if now.duration_since(since) < Duration::from_secs(30) => {}
                        _ => {
                            self.status.error = Some(format!("tracking lost: {e}"));
                            self.stop_guiding_quietly();
                            self.guide_hold = held; // pulses stop while blind; the mount keeps tracking on its own
                        }
                    }
                }
                self.dead_reckon(now);
            }
        }
    }

    /// Guides on a dead-reckoned position while the Sun cannot be seen, and gives up (pausing guiding) if that goes on too long
    /// or the corrections add up to too much to trust.
    fn dead_reckon(&mut self, now: Instant) {
        let Some(dr) = self.dead.as_ref() else { return };
        let GuideState::Guiding(g) = &self.guide else { self.dead = None; return };
        let (applied, target) = (g.applied(), g.target);
        if dr.age(now) > Duration::from_secs(15 * 60) || dr.moved_px(applied) > 300.0 || !self.mount_tracking {
            let held = self.calibration.map(|c| (target, c));
            self.event("dead_reckoning_gave_up", json!({ "age_s": dr.age(now).as_secs_f64(), "moved_px": dr.moved_px(applied) }));
            self.stop_guiding_quietly();
            self.guide_hold = held;
            self.status.error = Some(format!("tracking lost for too long: guiding paused until the {} is back", self.noun()));
            return;
        }
        let virt = dr.position(now, applied);
        self.step_guide(now, GuideInput::Pos(virt));
    }

    /// The recording finished by itself and the owner asked for the mount to go home afterwards: stop guiding and send it.
    fn home_after_capture(&mut self) {
        if self.mount.is_none() {
            return;
        }
        if self.flip.is_some() || self.goto.is_some() {
            self.status.error = Some("the recording finished but a flip or GoTo is running: the mount was not sent home".into());
            return;
        }
        if let Err(e) = self.require_not_parked() {
            self.event("home_when_done_skipped", json!({ "why": e }));
            return;
        }
        self.stop_guiding_quietly();
        match self.mount.as_mut().map(|m| m.go_home().map_err(s)) {
            Some(Ok(())) => {
                self.mount_tracking = false; // the mount stops tracking to go home; the next poll confirms
                self.event("home_when_done", json!({}));
                log::info!("recording finished: sending the mount home");
            }
            Some(Err(e)) => {
                self.event("home_when_done_failed", json!({ "why": e }));
                self.status.error = Some(format!("the recording finished but the mount could not be sent home: {e}"));
            }
            None => {}
        }
    }

    /// Seconds until the running recording ends, as far as can be told: `None` when it runs until stopped.
    fn capture_eta(&self) -> Option<f64> {
        let Job::Capture(job) = &self.job else { return None };
        let total = job.total_bursts?;
        let now = Instant::now();
        let fps = self.status.camera.as_ref().map_or(30.0, |c| c.fps.max(1.0));
        let spec_burst = || match (job.burst_seconds, job.burst_frames) {
            (Some(sec), Some(n)) => sec.min(n as f64 / fps),
            (Some(sec), None) => sec,
            (None, Some(n)) => n as f64 / fps,
            _ => 0.0,
        };
        let avg_burst = if job.bursts_done > 0 { job.burst_time_sum / job.bursts_done as f64 } else { spec_burst() };
        let (rem_current, started_at, next_wait) = match &job.phase {
            CapPhase::Capturing { started, taken, .. } => {
                let el = started.elapsed().as_secs_f64();
                let by_secs = job.burst_seconds.map(|sec| sec - el);
                let rate = if el > 0.5 && *taken > 0 { *taken as f64 / el } else { fps };
                let by_frames = job.burst_frames.map(|n| n.saturating_sub(*taken) as f64 / rate);
                let rem = match (by_secs, by_frames) { (Some(a), Some(b)) => a.min(b), (Some(a), None) => a, (None, Some(b)) => b, _ => 0.0 };
                (rem.max(0.0), Some(*started), None)
            }
            CapPhase::Waiting { next_at } => (0.0, None, Some(next_at.saturating_duration_since(now).as_secs_f64())),
        };
        let left = total.saturating_sub(job.burst_no) as f64; // bursts not yet started
        if let Some(l) = &job.lanes {
            let n = l.specs.len().max(1) as u32;
            let cycles_total = (total / n).max(1);
            let pos = (job.burst_no.saturating_sub(1)) % n; // index of the current (or just finished) burst in its cycle
            let in_cycle_after = (n - 1 - pos) as f64;
            let cycle_idx = (job.burst_no.saturating_sub(1)) / n;
            let full_after = cycles_total.saturating_sub(cycle_idx + 1) as f64;
            let one_cycle = avg_burst * n as f64;
            return Some(match (l.cycle_interval, next_wait) {
                (Some(c), _) if full_after > 0.0 => {
                    let to_next_cycle = (c.as_secs_f64() - l.cycle_started.elapsed().as_secs_f64()).max(0.0);
                    to_next_cycle + (full_after - 1.0) * c.as_secs_f64() + one_cycle
                }
                (None, Some(w)) => w + left * avg_burst,
                _ => rem_current + in_cycle_after * avg_burst + full_after * one_cycle,
            });
        }
        match (job.interval, started_at, next_wait) {
            (Some(i), Some(st), _) if left > 0.0 => Some((st + i).saturating_duration_since(now).as_secs_f64() + (left - 1.0) * i.as_secs_f64() + avg_burst),
            (Some(i), None, Some(w)) if left > 0.0 => Some(w + (left - 1.0) * i.as_secs_f64() + avg_burst),
            _ => Some(rem_current + left * avg_burst),
        }
    }

    /// Always-on event log: one JSON line per tracking, guiding, exposure or flip event in `<storage>/logs/<date>/events.jsonl`
    /// (small: events, not frames). Explains afterwards what the software did and when. Capped at 20 MB a day.
    fn event(&self, kind: &str, detail: Value) {
        let now = chrono::Utc::now();
        let path = self.storage.internal_log_dir().join(now.format("%Y-%m-%d").to_string()).join("events.jsonl");
        let ctl = |id: &str| self.status.camera.as_ref().and_then(|c| c.controls.iter().find(|c| c.id == id)).map(|c| c.value);
        let line = json!({ "t": now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true), "event": kind, "detail": detail,
            "exposure_us": ctl(CTRL_EXPOSURE_US), "gain": ctl("gain"), "capture": self.status.capture.state,
            "guide": match &self.guide { GuideState::Off => "off", GuideState::Calibrating(_) => "calibrating", GuideState::Guiding(_) => "guiding" },
            "mount_tracking": self.mount_tracking });
        self.alog.append(path, line.to_string(), Some(20 * 1024 * 1024)); // the writer thread does the disk work
    }

    /// Keeps the frame the tracker could not use, with what the software knew at that moment, in
    /// `<storage>/logs/<date>/lost/`. At most one per 10 s and 100 per run, so a flapping tracker cannot fill the drive.
    fn save_lost_frame(&mut self, frame: &Frame, why: &str) {
        if !self.save_lost || self.lost_saved >= 100 || self.last_lost_saved.is_some_and(|t| t.elapsed() < Duration::from_secs(10)) {
            return;
        }
        self.last_lost_saved = Some(Instant::now());
        if self.save_frame_note(frame, "lost", why).is_some() {
            self.lost_saved += 1;
        }
    }

    /// Writes the frame as PNG plus a JSON note of what the software knew into `logs/<date>/<kind>/`; returns the path (relative to storage).
    fn save_frame_note(&mut self, frame: &Frame, kind: &str, why: &str) -> Option<String> {
        let now = chrono::Utc::now();
        let day = now.format("%Y-%m-%d").to_string();
        let dir = self.storage.dir().join("logs").join(&day).join(kind);
        std::fs::create_dir_all(&dir).ok()?;
        let name = format!("{kind}_{}", now.format("%H%M%S%3fZ")); // no dot: with_extension would cut it
        let stem = dir.join(&name);
        let ctl = |id: &str| self.status.camera.as_ref().and_then(|c| c.controls.iter().find(|c| c.id == id)).map(|c| c.value);
        let lane = match &self.job {
            Job::Capture(j) => j.lanes.as_ref().map(|l| json!({ "name": l.specs[l.current].name, "mask_disc": l.specs[l.current].mask_disc, "skip_frames_left": l.skip, "burst": j.burst_no })),
            _ => None,
        };
        let note = json!({
            "time_utc": now.to_rfc3339(), "why": why,
            "frame": { "w": frame.width, "h": frame.height, "mean": frame.mean_fraction(), "saturated": frame.saturated_fraction() },
            "exposure_us": ctl(CTRL_EXPOSURE_US), "gain": ctl("gain"),
            "seconds_since_exposure_change": self.last_exposure_change.elapsed().as_secs_f64(),
            "tracking": self.status.tracking.as_ref().map(|t| json!({ "mode": t.mode, "x": t.x, "y": t.y, "radius": t.radius })),
            "sun": self.status.sun.as_ref().map(|s| json!({ "x": s.pos.x, "y": s.pos.y, "r": s.radius, "partial": s.partial })),
            "coronagraph": { "enabled": self.status.coronagraph.enabled, "scale": self.status.coronagraph.scale },
            "capture": self.status.capture.state, "lane": lane,
            "guiding": match &self.guide { GuideState::Off => "off", GuideState::Calibrating(_) => "calibrating", GuideState::Guiding(_) => "guiding" },
            "engine": self.debug_state(),
        });
        if rustflare_capture::write_png(&stem.with_extension("png"), frame).is_ok() {
            let _ = std::fs::write(stem.with_extension("json"), serde_json::to_vec_pretty(&note).unwrap_or_default());
            log::info!("{kind}: frame saved to {}", stem.with_extension("png").display());
            return Some(format!("logs/{day}/{kind}/{name}"));
        }
        None
    }

    /// Engine internals that are not in the status, for debugging (`GET /api/debug/state`).
    fn debug_state(&self) -> Value {
        let lanes = match &self.job {
            Job::Capture(j) => j.lanes.as_ref().map(|l| json!({ "current": l.current, "reference": reference_lane(&l.specs), "skip_frames_left": l.skip, "burst": j.burst_no, "names": l.specs.iter().map(|x| x.name.clone()).collect::<Vec<_>>() })),
            _ => None,
        };
        json!({
            "job": match &self.job { Job::Idle => "idle", Job::Capture(_) => "capture", _ => "other" },
            "lanes": lanes, "lost": self.lost, "guide_hold": self.guide_hold.map(|(t, _)| [t.x, t.y]),
            "seconds_since_exposure_change": self.last_exposure_change.elapsed().as_secs_f64(),
            "tracker_position": self.tracker.as_ref().map(|t| { let p = t.position(); json!({ "x": p.pos.x, "y": p.pos.y, "radius": p.radius }) }),
            "dead_reckoning": self.dead.is_some(), "unguided_drift_px_s": self.drift_model.drift().map(|d| [d.x, d.y]), "sun_held": self.sun_held, "learned_drift_px_s": self.sun_model.learned_drift().map(|d| [d.x, d.y]), "sun_for_mask": self.corona.lock().ok().and_then(|c| c.sun),
            "mount_tracking": self.mount_tracking, "fps": self.fps_ema, "save_lost_frames": self.save_lost, "lost_saved": self.lost_saved,
            "auto_flip": { "enabled": self.auto_flip.enabled, "recorded": self.auto_pre.is_some(), "done": self.auto_done, "late": self.auto_late },
            "flip_phase": self.flip.as_ref().map(|f| f.phase), "calibration_generation": self.cal_generation,
        })
    }

    /// One line per second into the session log (when it is on). Only technical state: no site, no device names.
    fn log_session(&mut self, frame: &Frame, now: Instant) {
        if !self.slog.due(now) {
            return;
        }
        let ctl = |id: &str| self.status.camera.as_ref().and_then(|c| c.controls.iter().find(|c| c.id == id)).map(|c| c.value);
        let rec = json!({
            "cam": { "fps": self.fps_ema, "mean": frame.mean_fraction(), "sat": frame.saturated_fraction(), "w": frame.width, "h": frame.height, "exposure_us": ctl(CTRL_EXPOSURE_US), "gain": ctl("gain") },
            "sun": self.status.sun.as_ref().map(|s| json!({ "x": s.pos.x, "y": s.pos.y, "r": s.radius, "partial": s.partial, "fit_rms": s.fit_rms })),
            "track": self.status.tracking.as_ref().map(|t| json!({ "mode": t.mode, "x": t.x, "y": t.y, "lost": t.lost })),
            "guide": { "state": match &self.guide { GuideState::Off => "off", GuideState::Calibrating(_) => "calibrating", GuideState::Guiding(_) => "guiding" }, "error_px": self.status.guiding.error_px },
            "mount": self.status.mount.as_ref().map(|m| json!({ "tracking": m.tracking, "slewing": m.slewing })),
            "capture": self.status.capture.state,
        });
        self.slog.write(now, rec, frame);
        if let Some(e) = self.slog.error.take() {
            self.event("session_log_stopped", json!({ "why": e }));
            self.status.error = Some(format!("the session log stopped: {e} (was the drive removed?)"));
        }
        if self.slog.capped {
            self.slog.capped = false;
            self.status.error = Some("session log stopped: it reached its 200 MB size cap".into());
        }
    }

    fn step_guide(&mut self, now: Instant, input: GuideInput) {
        let mut cmd = None;
        let mut failure = None;
        match &mut self.guide {
            GuideState::Off => {}
            GuideState::Guiding(g) => {
                match input {
                    GuideInput::Pos(pos) => {
                        cmd = g.step(now, pos);
                        // show the averaged offset the guider acts on (the instantaneous one is mostly seeing)
                        self.status.guiding.error_px = Some(g.filtered_error().map_or((g.target - pos).len(), |e| e.len()));
                    }
                    GuideInput::Tick => cmd = g.tick(now),
                    GuideInput::Judge => {
                        cmd = g.judge_now(now);
                        if let Some(e) = g.filtered_error() { self.status.guiding.error_px = Some(e.len()); }
                    }
                }
            }
            GuideState::Calibrating(c) => match (if let GuideInput::Pos(pos) = input { c.step(now, pos) } else { CalAction::None }) {
                CalAction::None => {}
                CalAction::Guide(g) => cmd = Some(g),
                CalAction::Done(cal) => {
                    self.event("calibration_done", json!(cal));
                    self.calibration = Some(cal);
                    self.cal_generation += 1;
                    self.guide = GuideState::Off;
                    cmd = Some(GuideCommand::STOP);
                }
                CalAction::Failed(msg) => {
                    self.event("calibration_failed", json!({ "why": msg.to_string() }));
                    failure = Some(format!("calibration failed: {msg}"));
                    self.guide = GuideState::Off;
                    cmd = Some(GuideCommand::STOP);
                }
            },
        }
        if let Some(c) = cmd {
            self.pulsing = c != GuideCommand::STOP;
            if !self.pulsing { self.pulse_settle_until = now + Duration::from_millis(1500); }
            self.slog.note_pulse(c.primary, c.secondary);
            let r = match self.mount.as_mut() {
                Some(m) => m.guide(RadPerSec(c.primary * SIDEREAL_RATE.0), RadPerSec(c.secondary * SIDEREAL_RATE.0)).map_err(s),
                None => Err("mount disconnected".to_string()),
            };
            if let Err(e) = r {
                failure = Some(format!("guiding stopped: {e}"));
                self.guide = GuideState::Off;
            }
        }
        if let Some(f) = failure {
            log::warn!("{f}");
            self.status.error = Some(f);
        }
    }

    // ------------------------------------------------------------------- jobs

    fn new_stem(&self, kind: &str, name: Option<&str>) -> PathBuf {
        let now = chrono::Utc::now();
        let dir = self.data_dir().join(now.format("%Y-%m-%d").to_string());
        let mut file = format!("{kind}_{}", now.format("%H%M%SZ"));
        if let Some(n) = name.map(sanitize).filter(|n| !n.is_empty()) {
            file = format!("{file}_{n}");
        }
        dir.join(file)
    }

    fn set_capture(&mut self, state: &'static str, phase: &str, total: Option<u32>) {
        self.status.capture.state = state;
        self.status.capture.phase = phase.into();
        self.status.capture.done = 0;
        self.status.capture.total = total;
        self.status.capture.file = None;
        self.status.capture.burst = None;
        self.status.capture.bursts_total = None;
        self.status.capture.next_in_s = None;
    }

    fn finish_capture(&mut self, file: Option<String>, note: &str) {
        self.event("capture_end", json!({ "note": note, "file": file }));
        let c = &mut self.status.capture;
        c.state = "idle";
        c.phase = note.into();
        c.file = None;
        if file.is_some() {
            c.last_file = file;
        }
        self.job = Job::Idle;
    }

    fn abort_job(&mut self, why: &str) {
        self.event("capture_abort", json!({ "why": why, "job": match &self.job { Job::Idle => "idle", Job::Capture(_) => "capture", Job::Single { .. } => "single", Job::Prominence(_) => "prominence", Job::Darks(_) => "darks", Job::Flats(_) => "flats" } }));
        let job = std::mem::replace(&mut self.job, Job::Idle);
        match job {
            Job::Idle | Job::Single { .. } => {}
            Job::Prominence(p) => self.set_exposure(p.original_exposure), // never leave the long exposure on the camera
            Job::Darks(_) => {}
            Job::Capture(job) => {
                self.restore_lanes(&job);
                let CaptureJob { sink, sink_no, phase, kind, name, burst_no, .. } = *job;
                if let CapPhase::Capturing { stem, taken, started } = phase {
                    sink.end();
                    let sc = self.sidecar(if kind == "burst" { "burst" } else { "sequence" }, json!({ "name": name, "burst": burst_no, "frames": taken, "duration_s": started.elapsed().as_secs_f64(), "stopped_early": why }));
                    self.write_sidecar(&stem, &sc);
                }
                sink.close();
                self.draining_no = sink_no;
                self.draining = Some(sink);
            }
            Job::Flats(f) => {
                if let FlatPhase::Collect { writer, .. } = f.phase {
                    let _ = writer.finalize();
                }
                self.set_exposure(f.original_exposure);
            }
        }
        self.finish_capture(None, why);
    }

    fn set_exposure(&mut self, us: f64) {
        self.last_exposure_change = Instant::now();
        self.event("exposure", json!({ "us": us }));
        if let Some(cam) = self.camera.as_mut() {
            if cam.set_control(CTRL_EXPOSURE_US, us).is_ok() {
                if let Some(c) = self.status.camera.as_mut().and_then(|c| c.controls.iter_mut().find(|c| c.id == CTRL_EXPOSURE_US)) {
                    c.value = us.clamp(c.min, c.max);
                }
            }
        }
    }

    fn exposure(&self) -> f64 {
        self.status.camera.as_ref().and_then(|c| c.controls.iter().find(|c| c.id == CTRL_EXPOSURE_US)).map(|c| c.value).unwrap_or(0.0)
    }

    fn sidecar(&self, kind: &str, extra: Value) -> Value {
        let cam = self.status.camera.as_ref();
        json!({
            "kind": kind,
            "written_utc": chrono::Utc::now().to_rfc3339(),
            "camera": cam.map(|c| &c.name),
            "controls": cam.map(|c| c.controls.iter().map(|c| (c.id.clone(), c.value)).collect::<std::collections::BTreeMap<_, _>>()),
            "mount": self.mount.as_ref().map(|m| m.info()),
            "mount_tracking": self.mount_tracking,
            "tracking": self.status.tracking,
            "details": extra,
        })
    }

    fn write_sidecar(&self, stem: &std::path::Path, v: &Value) {
        if let Some(dir) = stem.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(stem.with_extension("json"), serde_json::to_vec_pretty(v).unwrap_or_default());
    }

    fn step_job(&mut self, frame: Frame, mean: f64) {
        let job = std::mem::replace(&mut self.job, Job::Idle);
        self.job = match job {
            Job::Idle => Job::Idle,
            Job::Single { stem, fits } => {
                let path = stem.with_extension(if fits { "fits" } else { "png" });
                let ctl = |id: &str| self.status.camera.as_ref().and_then(|c| c.controls.iter().find(|c| c.id == id)).map(|c| c.value);
                let meta = rustflare_capture::FitsMeta { instrument: self.status.camera.as_ref().map(|c| c.name.clone()), exposure_us: ctl(CTRL_EXPOSURE_US), gain: ctl("gain"), object: crate::status::profile_object(self.status.profile), comments: vec![] };
                let gains = { let c = self.colour.lock().unwrap(); match c.mode { "off" => [1.0; 3], "manual" => [c.r, 1.0, c.b], _ => frame.bayer.map(|p| rustflare_capture::debayer::auto_white_balance(&frame, p)).unwrap_or([1.0; 3]) } };
                let r = std::fs::create_dir_all(path.parent().unwrap()).map_err(s).and_then(|_| {
                    if fits {
                        rustflare_capture::write_fits(&path, &frame, &meta).map_err(s)
                    } else if let Some(p) = frame.bayer {
                        // a colour camera: the raw mosaic as <name>_raw.png and the debayered, white-balanced picture as <name>.png
                        write_png(&stem.with_file_name(format!("{}_raw", stem.file_name().unwrap().to_string_lossy())).with_extension("png"), &frame).map_err(s)?;
                        rustflare_capture::write_png_rgb(&path, &rustflare_capture::debayer::bilinear(&frame, p, gains)).map_err(s)
                    } else {
                        write_png(&path, &frame).map_err(s)
                    }
                });
                match r {
                    Ok(()) => {
                        let sc = self.sidecar("single", json!({ "width": frame.width, "height": frame.height }));
                        self.write_sidecar(&stem, &sc);
                        self.finish_capture(Some(rel(&self.data_dir(), &path)), "saved");
                    }
                    Err(e) => {
                        self.status.error = Some(format!("saving frame failed: {e}"));
                        self.finish_capture(None, "failed");
                    }
                }
                Job::Idle
            }
            Job::Capture(job) => self.step_capture(job, frame),
            Job::Prominence(p) => self.step_prominence(p, frame),
            Job::Darks(d) => self.step_darks(d, frame),
            Job::Flats(f) => {
                let m = frame.mean_fraction(); // flats need a fresh level for every frame
                let _ = mean;
                self.step_flats(f, &frame, m)
            }
        };
    }

    fn step_capture(&mut self, mut job: Box<CaptureJob>, mut frame: Frame) -> Job {
        let now = Instant::now();
        let total_txt = job.total_bursts.map(|t| format!("/{t}")).unwrap_or_default();
        if let CapPhase::Waiting { next_at } = job.phase {
            if now < next_at {
                self.status.capture.next_in_s = Some((next_at - now).as_secs_f64());
                self.status.capture.phase = format!("waiting {:.0} s for burst {}{total_txt}", (next_at - now).as_secs_f64().ceil(), job.burst_no + 1);
                return Job::Capture(job);
            }
            job.burst_no += 1;
            let mut lane_label = None;
            let mut stem = if let Some(l) = job.lanes.as_mut() {
                let n = l.specs.len() as u32;
                let spec = l.specs[((job.burst_no - 1) % n) as usize].clone();
                let cycle = (job.burst_no - 1) / n + 1;
                if (job.burst_no - 1) % n == 0 {
                    l.cycle_started = Instant::now();
                }
                job.burst_frames = spec.frames;
                job.burst_seconds = spec.seconds;
                l.skip = 2;
                self.last_exposure_change = Instant::now();
                l.current = ((job.burst_no - 1) % n) as usize;
                self.set_exposure(spec.exposure_us);
                if let (Some(g), Some(cam)) = (spec.gain, self.camera.as_mut()) {
                    let _ = cam.set_control("gain", g);
                }
                self.event("lane", json!({ "name": spec.name, "exposure_us": spec.exposure_us, "gain": spec.gain, "cycle": cycle, "mask_disc": spec.mask_disc }));
                lane_label = Some(spec.name.clone());
                let l = job.lanes.as_ref().unwrap();
                l.dir.join(format!("lane-{}", sanitize(&spec.name))).join(format!("{}_c{cycle:03}", sanitize(&spec.name)))
            } else {
                self.new_stem(job.kind, Some(&job.name))
            };
            if job.kind == "burst" {
                stem = stem.with_file_name(format!("{}_b{:03}", stem.file_name().unwrap().to_string_lossy(), job.burst_no));
            }
            let instrument = self.status.camera.as_ref().map(|c| c.name.clone()).unwrap_or_default();
            let ctl = |id: &str| self.status.camera.as_ref().and_then(|c| c.controls.iter().find(|c| c.id == id)).map(|c| c.value);
            let fits_meta = rustflare_capture::FitsMeta {
                instrument: Some(instrument.clone()),
                exposure_us: ctl(CTRL_EXPOSURE_US),
                gain: ctl("gain"),
                object: crate::status::profile_object(self.status.profile),
                comments: vec![format!("flat applied to preview only: {}", if self.status.flats.applied { "yes" } else { "no" }), "frames are raw".into()],
            };
            let spec = BurstSpec { stem: stem.clone(), format: job.format, instrument, max_file_bytes: job.max_file, fits_meta };
            if self.plan_burst(&mut job) {
                job.sink.begin_deferred(spec);
            } else if job.cur.is_some() {
                job.sink.begin_kept(spec); // written as usual, but kept in the ring for the stacker
            } else {
                job.sink.begin(spec);
            }
            self.status.capture.burst = Some(job.burst_no);
            if let Some(n) = lane_label {
                self.status.capture.phase = format!("lane {n}");
            }
            self.status.capture.next_in_s = None;
            self.status.capture.done = 0;
            self.status.capture.file = Some(rel(&self.data_dir(), &stem));
            job.disc_track.clear();
            job.phase = CapPhase::Capturing { stem, started: now, taken: 0 };
        }
        if let Some(l) = job.lanes.as_mut() {
            if l.skip > 0 {
                l.skip -= 1; // frames already in flight carry the previous lane's exposure
                return Job::Capture(job);
            }
        }
        // coronagraph lane: the frame is saved raw; remember where the model puts the disc in it (blacked out after stacking)
        let frame_bytes = frame.data.len() as u64;
        let disc_pos = job.lanes.as_ref().filter(|l| l.specs[l.current].mask_disc).map(|_| match self.status.sun.as_ref() {
            Some(s) => [(s.pos.x * 10.0).round() as f32 / 10.0, (s.pos.y * 10.0).round() as f32 / 10.0, (s.radius * 10.0).round() as f32 / 10.0],
            None => [0.0, 0.0, 0.0],
        });
        let CapPhase::Capturing { stem, started, taken } = &mut job.phase else { unreachable!() };
        if job.drop_when_full {
            match job.sink.try_push(frame) {
                None => return self.fail_capture(job, "the disk writer stopped unexpectedly".into()),
                Some(false) => {
                    self.status.capture.dropped = job.sink.dropped();
                    return Job::Capture(job); // not counted: only frames that reach the disk count toward the goal
                }
                Some(true) => {}
            }
        } else if !job.sink.push(frame) {
            return self.fail_capture(job, "the disk writer stopped unexpectedly".into());
        }
        *taken += 1;
        if let Some(p) = disc_pos {
            job.disc_track.push(p);
        }
        self.status.capture.done = *taken;
        self.status.capture.phase = format!("burst {}{total_txt}: capturing", job.burst_no);

        let mut stop: Option<String> = job.sink.error().map(|e| format!("writing failed: {e}"));
        if stop.is_none() && job.last_check.elapsed() >= Duration::from_secs(2) {
            job.last_check = now;
            stop = self.storage.running_problem(job.sink.queued() as u64 * frame_bytes);
        }
        let burst_done = stop.is_some()
            || job.burst_frames.is_some_and(|n| *taken >= n)
            || job.burst_seconds.is_some_and(|sec| started.elapsed().as_secs_f64() >= sec);
        if !burst_done {
            return Job::Capture(job);
        }

        let (stem, taken, started) = (stem.clone(), *taken, *started);
        let unseen = job.disc_track.iter().filter(|p| p[2] <= 0.0).count();
        if unseen > 0 {
            self.event("disc_position_missing", json!({ "frames": unseen, "of": job.disc_track.len() }));
            log::warn!("coronagraph lane: the Sun was not in view in {unseen} of {} frames: those have no disc position", job.disc_track.len());
        }
        let in_ram = job.sink.queued(); // frames still waiting for the disk: gone if the drive is
        let (end_id, end_first, end_last) = job.sink.end();
        if let Some(cur) = job.cur.take() {
            if stop.is_some() || taken == 0 {
                // a burst that ended badly: its video is written, never held back for a stack
                if cur.deferred {
                    job.sink.resolve(end_id, Resolution::WriteAll);
                }
            } else {
                self.submit_stack(&job, end_id, end_first, end_last, &stem, cur);
            }
        }
        let sc = self.sidecar(
            if job.kind == "burst" { "burst" } else { "sequence" },
            json!({ "name": job.name, "burst": job.burst_no, "of": job.total_bursts, "frames": taken,
                    "format": match job.format { Format::Png => "png", Format::Fits => "fits", Format::Ser => "ser" },
                    "dropped_frames": job.sink.dropped(),
                    "duration_s": started.elapsed().as_secs_f64(), "stopped_early": stop,
                    "disc_track": if job.disc_track.is_empty() { Value::Null } else { json!(job.disc_track) } }),
        );
        self.write_sidecar(&stem, &sc);
        if let Some(why) = stop {
            let note = format!("The recording stopped on its own: {why}. {taken} frames were captured, and about {in_ram} that were still in memory may not have reached the drive.");
            self.event("capture_stopped_early", json!({ "why": why, "frames": taken, "frames_in_ram": in_ram }));
            self.notice("error", note.clone());
            self.status.capture.stopped_early = Some(note);
            self.status.error = Some(format!("recording stopped: {why} ({taken} frames in this burst)"));
            return self.fail_capture_keep(job);
        }
        job.bursts_done += 1;
        job.burst_time_sum += started.elapsed().as_secs_f64();
        let last = (job.interval.is_none() && job.lanes.is_none()) || job.total_bursts.is_some_and(|t| job.burst_no >= t);
        if last {
            self.restore_lanes(&job);
            let home = job.home_when_done;
            let CaptureJob { sink, sink_no, .. } = *job;
            self.draining_no = sink_no;
            if self.stack_jobs.values().any(|j| j.sink_no == sink_no) {
                self.close_when_stacked = true; // bursts are still being stacked from memory: the sink stays open for them
            } else {
                sink.close();
            }
            self.draining = Some(sink);
            self.finish_capture(None, "captured; writing buffered frames to disk");
            if home {
                self.home_after_capture();
            }
            return Job::Idle;
        }
        let next_at = if let Some(l) = &job.lanes {
            // time-lapse cadence: after the last lane of a cycle, wait for the next cycle slot
            match l.cycle_interval {
                Some(i) if job.burst_no % l.specs.len() as u32 == 0 => (l.cycle_started + i).max(Instant::now()),
                _ => Instant::now(),
            }
        } else { (started + job.interval.unwrap()).max(Instant::now()) };
        job.phase = CapPhase::Waiting { next_at };
        Job::Capture(job)
    }

    /// Puts the exposure and gain back to what they were before the lanes started.
    fn restore_lanes(&mut self, job: &CaptureJob) {
        if let Some(l) = &job.lanes {
            self.set_exposure(l.original.0);
            if let (Some(g), Some(cam)) = (l.original.1, self.camera.as_mut()) {
                let _ = cam.set_control("gain", g);
            }
        }
    }

    /// Ends a capture job after an error, keeping whatever was already written.
    fn fail_capture(&mut self, job: Box<CaptureJob>, why: String) -> Job {
        self.status.error = Some(why);
        self.fail_capture_keep(job)
    }

    fn fail_capture_keep(&mut self, job: Box<CaptureJob>) -> Job {
        self.restore_lanes(&job);
        let CaptureJob { sink, sink_no, .. } = *job;
        sink.close();
        self.draining_no = sink_no;
        self.draining = Some(sink);
        self.finish_capture(None, "stopped");
        Job::Idle
    }

    /// Collects finished files from the writer(s) and retires a drained sink.
    fn poll_sink(&mut self) {
        let mut finished = vec![];
        let mut err = None;
        if let Job::Capture(j) = &self.job {
            finished.extend(j.sink.take_finished());
        }
        let mut done = false;
        if let Some(d) = &self.draining {
            finished.extend(d.take_finished());
            err = d.error();
            done = d.queued() == 0 && d.is_finished();
            if done && d.rate_bytes_per_s() > 0.0 {
                self.last_disk_mb_s = d.rate_bytes_per_s() / 1e6;
            }
        }
        if done {
            self.draining = None;
            if matches!(self.job, Job::Idle) {
                self.status.capture.state = "idle";
                self.status.capture.phase = "saved".into();
            }
        }
        for f in finished {
            if f.dropped > 0 {
                // never quiet: an event, a persistent note, an error line now, and the burst's sidecar says so
                let file = rel(&self.data_dir(), &f.path);
                let note = if f.frames == 0 {
                    format!("All {} frames of {file} were lost and no file was written: the drive could not keep up with the camera and the RAM buffer was full, so the oldest waiting frames were overwritten (\"drop frames\" mode).", f.dropped)
                } else {
                    format!("{} of the frames of {file} were lost: the drive could not keep up with the camera and the RAM buffer was full, so the oldest waiting frames were overwritten (\"drop frames\" mode). The file holds {} frames.", f.dropped, f.frames)
                };
                self.event("frames_dropped", json!({ "file": file, "dropped": f.dropped, "written": f.frames }));
                self.status.error = Some(note.clone());
                self.notice("error", note.clone());
                self.status.capture.frames_lost = Some(note);
                let side = f.path.with_extension("json");
                if let Some(mut v) = std::fs::read_to_string(&side).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
                    v["details"]["dropped_frames"] = json!(f.dropped);
                    v["details"]["frames_written"] = json!(f.frames);
                    self.alog.replace(side, serde_json::to_string_pretty(&v).unwrap_or_default());
                }
            }
            if f.frames > 0 {
                self.status.capture.last_file = Some(rel(&self.data_dir(), &f.path));
            }
            if f.parts > 1 {
                log::info!("{} was split into {} files (filesystem size limit)", f.path.display(), f.parts);
            }
        }
        if let Some(e) = err {
            if self.status.error.is_none() {
                self.status.error = Some(format!("writing to disk failed: {e}"));
            }
        }
    }

    fn step_flats(&mut self, mut f: Box<Flats>, frame: &Frame, mean: f64) -> Job {
        match &mut f.phase {
            FlatPhase::Adjust { iter, skip } => {
                if *skip > 0 {
                    *skip -= 1; // frames already in flight were exposed with the old setting
                    return Job::Flats(f);
                }
                let sat = frame.saturated_fraction();
                let exp = self.exposure();
                match f.target.decide(*iter, exp, mean, sat) {
                    FlatDecision::Accept => {
                        f.exposure_used = exp;
                        let path = f.stem.with_extension("ser");
                        let cam = self.status.camera.as_ref().map(|c| c.name.clone()).unwrap_or_default();
                        let r = std::fs::create_dir_all(path.parent().unwrap()).map_err(s).and_then(|_| SerWriter::create_color(&path, frame.width, frame.height, frame.format, frame.bayer, &cam, "Rustflare flats").map_err(s));
                        match r {
                            Ok(writer) => {
                                f.phase = FlatPhase::Collect { writer, acc: FlatAccumulator::new(frame.width, frame.height, frame.format) };
                                self.status.capture.phase = format!("collecting at {exp:.0} µs ({:.0}%)", mean * 100.0);
                                self.status.capture.file = Some(rel(&self.data_dir(), &path));
                                // this frame is already at the right level: use it
                                return self.step_flats(f, frame, mean);
                            }
                            Err(e) => return self.fail_flats(f, format!("cannot start flats: {e}")),
                        }
                    }
                    FlatDecision::SetExposure(e) => {
                        *iter += 1;
                        *skip = 2;
                        self.status.capture.phase = format!("finding exposure ({iter}): {:.0}% → {:.0} µs", mean * 100.0, e);
                        self.set_exposure(e);
                        Job::Flats(f)
                    }
                    FlatDecision::GiveUp(msg) => self.fail_flats(f, format!("flats: {msg}")),
                }
            }
            FlatPhase::Collect { writer, acc } => {
                let r = writer.write_frame(frame).and(acc.add(frame));
                if let Err(e) = r {
                    return self.fail_flats(f, format!("flats failed: {e}"));
                }
                self.status.capture.done = acc.count();
                if acc.count() < f.frames {
                    return Job::Flats(f);
                }
                let Flats { stem, frames, exposure_used, original_exposure, target, phase } = *f;
                let FlatPhase::Collect { writer, acc } = phase else { unreachable!() };
                let n = writer.finalize().unwrap_or(frames);
                let master = stem.with_file_name(format!("{}_master", stem.file_name().unwrap().to_string_lossy())).with_extension("png");
                let master_frame = acc.master().map_err(s);
                let master_res = master_frame.as_ref().map_err(|e| e.clone()).and_then(|m| write_png(&master, m).map_err(s));
                if let (Ok(m), Ok(())) = (&master_frame, &master_res) {
                    let rel_png = rel(&self.data_dir(), &master);
                    self.install_flat(m, rel_png);
                }
                self.set_exposure(original_exposure);
                if let Err(e) = master_res {
                    self.status.error = Some(format!("master flat failed: {e}"));
                }
                let sc = self.sidecar("flats", json!({ "frames": n, "exposure_us": exposure_used, "target_level": target.level, "final_mean": mean, "master": master.file_name().map(|n| n.to_string_lossy().to_string()), "restored_exposure_us": original_exposure }));
                self.write_sidecar(&stem, &sc);
                self.finish_capture(Some(rel(&self.data_dir(), &stem.with_extension("ser"))), "saved");
                Job::Idle
            }
        }
    }

    fn step_darks(&mut self, mut d: Box<Darks>, frame: Frame) -> Job {
        if d.skip > 0 {
            d.skip -= 1;
            return Job::Darks(d);
        }
        if d.raw.is_none() {
            let path = d.stem.with_extension("ser");
            let cam = self.status.camera.as_ref().map(|c| c.name.clone()).unwrap_or_default();
            match std::fs::create_dir_all(path.parent().unwrap()).map_err(s).and_then(|_| SerWriter::create_color(&path, frame.width, frame.height, frame.format, frame.bayer, &cam, "Rustflare darks").map_err(s)) {
                Ok(w) => d.raw = Some(w),
                Err(e) => log::warn!("the raw dark frames are not kept: {e}"), // the master is still made
            }
        }
        if let Some(w) = d.raw.as_mut() {
            if let Err(e) = w.write_frame(&frame) {
                log::warn!("writing a raw dark frame failed: {e}");
                d.raw = None;
            }
        }
        if let Err(e) = d.acc.add(&frame) {
            self.status.error = Some(format!("darks: {e}"));
            self.finish_capture(None, "failed");
            return Job::Idle;
        }
        self.status.capture.done = d.acc.count();
        self.status.capture.phase = "dark frames: keep the cap on".into();
        if d.acc.count() < d.frames {
            return Job::Darks(d);
        }
        if let Some(w) = d.raw.take() {
            let _ = w.finalize();
        }
        let master = d.stem.with_file_name(format!("{}_master", d.stem.file_name().unwrap().to_string_lossy())).with_extension("png");
        let res = std::fs::create_dir_all(master.parent().unwrap()).map_err(s).and_then(|_| d.acc.master().map_err(s)).and_then(|m| write_png(&master, &m).map_err(s).map(|_| m));
        match res {
            Ok(m) => {
                let rel_png = rel(&self.data_dir(), &master);
                match rustflare_capture::DarkFrame::from_master(&m) {
                    Ok(df) => {
                        {
                            let mut st = self.flat.lock().unwrap();
                            st.dark = Some(std::sync::Arc::new(df));
                            st.dark_enabled = true;
                            st.dark_name = Some(rel_png.clone());
                            st.dark_exposure_us = Some(d.exposure_us);
                            st.dark_gain = Some(d.gain);
                        }
                        self.save_flat_choice();
                        let sc = self.sidecar("darks", json!({ "frames": d.frames, "exposure_us": d.exposure_us, "gain": d.gain, "master": rel_png }));
                        self.write_sidecar(&d.stem, &sc);
                        self.finish_capture(Some(rel_png), "saved");
                    }
                    Err(e) => {
                        self.status.error = Some(format!("master dark unusable: {e}"));
                        self.finish_capture(None, "failed");
                    }
                }
            }
            Err(e) => {
                self.status.error = Some(format!("saving the master dark failed: {e}"));
                self.finish_capture(None, "failed");
            }
        }
        Job::Idle
    }

    fn step_prominence(&mut self, mut p: Box<Prominence>, frame: Frame) -> Job {
        let dir_ok = std::fs::create_dir_all(p.stem.parent().unwrap());
        if let Err(e) = dir_ok {
            self.status.error = Some(format!("prominence: {e}"));
            self.finish_capture(None, "failed");
            return Job::Idle;
        }
        match p.disc.take() {
            None => {
                let Some(sun) = self.status.sun.as_ref().map(|s| (s.pos.x, s.pos.y, s.radius)) else {
                    self.status.error = Some("prominence: the Sun was lost from the frame".into());
                    self.finish_capture(None, "failed");
                    return Job::Idle;
                };
                let max = self.status.camera.as_ref().and_then(|c| c.controls.iter().find(|c| c.id == CTRL_EXPOSURE_US)).map(|c| c.max).unwrap_or(2_000_000.0);
                p.long_exposure = (p.original_exposure * p.factor).min(max);
                p.disc = Some((frame, sun));
                p.skip = 4;
                self.set_exposure(p.long_exposure);
                self.set_capture("single", "long exposure", None);
                Job::Prominence(p)
            }
            Some(d) => {
                if p.skip > 0 {
                    p.skip -= 1;
                    p.disc = Some(d);
                    return Job::Prominence(p);
                }
                self.set_exposure(p.original_exposure);
                let (short, (cx, cy, r)) = d;
                let factor = p.long_exposure / p.original_exposure.max(1.0);
                let stem = p.stem.clone();
                let res = (|| -> Result<(), String> {
                    write_png(&stem.with_file_name(format!("{}_disc", stem.file_name().unwrap().to_string_lossy())).with_extension("png"), &short).map_err(s)?;
                    write_png(&stem.with_file_name(format!("{}_long", stem.file_name().unwrap().to_string_lossy())).with_extension("png"), &frame).map_err(s)?;
                    let comp = rustflare_capture::prominence::composite(&short, &frame, cx, cy, r, factor).map_err(s)?;
                    write_png(&stem.with_file_name(format!("{}_composite", stem.file_name().unwrap().to_string_lossy())).with_extension("png"), &comp).map_err(s)
                })();
                match res {
                    Ok(()) => {
                        let sc = self.sidecar("prominence", json!({ "disc_exposure_us": p.original_exposure, "long_exposure_us": p.long_exposure, "factor": factor, "sun": { "x": cx, "y": cy, "radius": r } }));
                        self.write_sidecar(&stem, &sc);
                        let name = format!("{}_composite.png", stem.file_name().unwrap().to_string_lossy());
                        let rel_path = rel(&self.data_dir(), &stem.with_file_name(name));
                        self.finish_capture(Some(rel_path), "saved");
                    }
                    Err(e) => {
                        self.status.error = Some(format!("prominence capture failed: {e}"));
                        self.finish_capture(None, "failed");
                    }
                }
                Job::Idle
            }
        }
    }

    /// What is in use now, as a setup would save it.
    fn current_use(&self) -> crate::setups::Current {
        let f = self.flat.lock().unwrap();
        crate::setups::Current { optics: self.optics.clone(), profile: self.status.profile.to_string(), flat: f.name.clone(), dark: f.dark_name.clone(), dark_exposure_us: f.dark_exposure_us, dark_gain: f.dark_gain }
    }

    /// Validates and installs the optics (the expected radius and the tracker follow), and saves them. `PUT /api/optics` and applying a setup both come here.
    fn set_optics(&mut self, o: OpticsSettings) -> Result<(), String> {
        crate::setups::check_optics(o.focal_length_mm, o.focal_multiplier, o.pixel_size_um, o.binning)?;
        self.optics = o;
        self.radius_samples.clear();
        self.measured_radius = None;
        crate::config::set(self.storage.config_file(), "optics", serde_json::to_value(&self.optics).unwrap_or_default())?;
        Ok(())
    }

    /// Applies a named optical setup: the optics, the profile it comes with, and its flat and dark when they are named and still there.
    /// What could not be restored is reported, never fatal. Nothing moves and nothing starts.
    fn apply_setup(&mut self, name: &str) -> Reply {
        // the optics and the profile end up in the headers of the frames being written: not while capturing
        if !matches!(self.job, Job::Idle) {
            return Err("stop the recording first: applying a setup changes the optics and the profile, which are written into the files being recorded".into());
        }
        let setup = self.setups.get(name)?.clone();
        let summary = |s: &Status| json!({ "profile": s.profile, "focal_length_mm": s.optics.focal_length_mm, "focal_multiplier": s.optics.focal_multiplier, "effective_focal_mm": s.optics.effective_focal_mm, "binning": s.optics.binning, "scale_arcsec_per_px": s.optics.scale_arcsec_per_px, "expected_radius_px": s.optics.expected_radius_px });
        self.refresh_optics();
        let before = summary(&self.status);
        self.set_optics(setup.optics())?;
        if let Some(p) = &setup.profile {
            if p != self.status.profile {
                self.handle(Command::SetProfile { id: p.clone() })?;
            }
        }
        let mut not_restored = vec![];
        if let Some(rel) = &setup.flat {
            let blocked = if matches!(self.job, Job::Flats(_)) { Err("flats are being captured".to_string()) } else { load_flat(&self.storage, &self.flat, rel, true) };
            if let Err(e) = blocked {
                not_restored.push(json!({ "what": "flat", "name": rel, "reason": e }));
            }
        }
        if let Some(rel) = &setup.dark {
            let keep = { let f = self.flat.lock().unwrap(); (f.dark_name.as_deref() == Some(rel.as_str())).then_some((f.dark_exposure_us, f.dark_gain)) };
            // the exposure and gain saved with the setup, else those of the dark already loaded
            let (exp, gain) = match (setup.dark_exposure_us, setup.dark_gain) { (None, None) => keep.unwrap_or((None, None)), saved => saved };
            let blocked = if matches!(self.job, Job::Darks(_)) { Err("darks are being captured".to_string()) } else { load_dark(&self.storage, &self.flat, rel, true, exp, gain) };
            if let Err(e) = blocked {
                not_restored.push(json!({ "what": "dark", "name": rel, "reason": e }));
            }
        }
        if setup.flat.is_some() || setup.dark.is_some() {
            self.save_flat_choice();
        }
        self.setups.active = Some(setup.name.clone());
        if let Err(e) = self.setups.persist(self.storage.config_file()) {
            not_restored.push(json!({ "what": "active setup", "name": setup.name, "reason": e }));
        }
        self.refresh_optics();
        self.event("optics_setup_applied", json!({ "name": setup.name, "profile": self.status.profile, "not_restored": not_restored }));
        Ok(json!({ "applied": setup.name, "before": before, "after": summary(&self.status), "not_restored": not_restored }))
    }

    fn save_flat_choice(&self) {
        let s = self.flat.lock().unwrap();
        if let Some(name) = &s.name {
            let _ = crate::config::set(self.storage.config_file(), "flat", json!({ "master": name, "enabled": s.enabled }));
        }
        if let Some(name) = &s.dark_name {
            let _ = crate::config::set(self.storage.config_file(), "dark", json!({ "master": name, "enabled": s.dark_enabled, "exposure_us": s.dark_exposure_us, "gain": s.dark_gain }));
        }
    }

    /// Installs a freshly captured master flat and turns the correction on.
    fn install_flat(&mut self, master: &Frame, rel_png: String) {
        match rustflare_capture::FlatField::from_master(master) {
            Ok(ff) => {
                {
                    let mut s = self.flat.lock().unwrap();
                    s.field = Some(std::sync::Arc::new(ff));
                    s.enabled = true;
                    s.name = Some(rel_png);
                }
                self.save_flat_choice();
            }
            Err(e) => self.status.error = Some(format!("master flat unusable: {e}")),
        }
    }

    fn fail_flats(&mut self, f: Box<Flats>, msg: String) -> Job {
        if let FlatPhase::Collect { writer, .. } = f.phase {
            let _ = writer.finalize();
        }
        self.set_exposure(f.original_exposure);
        self.status.error = Some(msg);
        self.finish_capture(None, "failed");
        Job::Idle
    }
}

/// Free memory in MB (Linux), for sizing the capture buffer.
fn mem_available_mb() -> Option<u64> {
    let t = std::fs::read_to_string("/proc/meminfo").ok()?;
    let kb: u64 = t.lines().find(|l| l.starts_with("MemAvailable:"))?.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024)
}

/// What the kernel's memory manager was doing, for a `frame_gap` event: pages waiting to be written (`dirty_mb`) and being written
/// (`writeback_mb`), how long tasks have waited for memory in the last 10 s (`psi_some_avg10`: some task stalled, `psi_full_avg10`:
/// all of them), and the count of times a task had to free memory itself instead of getting it (`allocstall`, rising during a gap
/// means direct reclaim, which is a stall inside whatever thread needed the memory). Issue #86.
/// Is the picture far darker than it has been for the last minute (a shadow over the telescope, thick cloud), with the exposure unchanged?
/// Then the limb fit would only find edges in noise, so nothing is measured and the Sun is reported lost at once.
fn picture_is_dark(settled: bool, reference: f64, mean: f64) -> bool {
    settled && reference > 0.01 && mean < 0.25 * reference
}

#[cfg(test)]
mod dark_tests {
    use super::picture_is_dark;
    #[test]
    fn a_picture_that_falls_to_a_quarter_of_its_usual_light_is_dark_but_not_while_the_exposure_just_changed() {
        assert!(picture_is_dark(true, 0.064, 0.003), "the 2026-11-26 shadow: 0.064 to 0.003");
        assert!(picture_is_dark(true, 0.4, 0.09));
        assert!(!picture_is_dark(true, 0.4, 0.2), "thin cloud halving the light is not darkness");
        assert!(!picture_is_dark(false, 0.4, 0.01), "just after an exposure change the level is meaningless");
        assert!(!picture_is_dark(true, 0.005, 0.0001), "no reference yet, or a night sky: nothing to compare with");
    }
}

fn memory_state() -> serde_json::Value {
    let kb = |t: &str, key: &str| t.lines().find(|l| l.starts_with(key)).and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok());
    let info = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let psi = std::fs::read_to_string("/proc/pressure/memory").unwrap_or_default();
    let avg10 = |kind: &str| psi.lines().find(|l| l.starts_with(kind)).and_then(|l| l.split_whitespace().find_map(|w| w.strip_prefix("avg10=")?.parse::<f64>().ok()));
    let vm = std::fs::read_to_string("/proc/vmstat").unwrap_or_default();
    let allocstall: u64 = vm.lines().filter(|l| l.starts_with("allocstall")).filter_map(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok()).sum();
    json!({ "dirty_mb": kb(&info, "Dirty:").map(|k| k / 1024), "writeback_mb": kb(&info, "Writeback:").map(|k| k / 1024),
            "psi_some_avg10": avg10("some"), "psi_full_avg10": avg10("full"), "allocstall": if vm.is_empty() { None } else { Some(allocstall) } })
}

fn rel(base: &std::path::Path, p: &std::path::Path) -> String {
    p.strip_prefix(base).unwrap_or(p).to_string_lossy().to_string()
}


// ---------------------------------------------------------------- after each burst: stacking from memory (ADR 0019)

const NO_STACKER: &str = "this recording is set to make stacks and not keep all the video, but the stacker is not running (systemctl status rustflare-stack). Start it, or choose \"Keep the video\" under Record > After each burst.";

/// What the recorder does with a burst once it is captured.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum AfterMode {
    /// 1: write the video; stacking, if wanted, happens later from the file.
    Video,
    /// 2: write the video and stack the burst from memory.
    VideoStack,
    /// 3: stack from memory and write no video.
    StackOnly,
    /// 4: stack from memory and write only the best frames.
    BestFrames,
}

impl AfterMode {
    fn id(self) -> &'static str {
        match self {
            AfterMode::Video => "video",
            AfterMode::VideoStack => "video_stack",
            AfterMode::StackOnly => "stack_only",
            AfterMode::BestFrames => "best_frames",
        }
    }
    fn from_id(s: &str) -> Option<AfterMode> {
        Some(match s {
            "video" => AfterMode::Video,
            "video_stack" => AfterMode::VideoStack,
            "stack_only" => AfterMode::StackOnly,
            "best_frames" => AfterMode::BestFrames,
            _ => return None,
        })
    }
    /// The burst's frames wait in the ring until the stack says what to write.
    fn defers(self) -> bool { matches!(self, AfterMode::StackOnly | AfterMode::BestFrames) }
}

#[derive(Clone, Debug)]
struct AfterBurst {
    mode: AfterMode,
    /// Percentage of the frames of a burst that go into its stack.
    stack_keep_percent: f64,
    /// Percentage of the frames written to disk in mode 4.
    raw_keep_percent: f64,
    /// A stack is planned to take this many times its measured time: the safety margin against the camera overwriting the burst.
    margin: f64,
}

impl AfterBurst {
    fn load(v: &Value) -> AfterBurst {
        let num = |k: &str, d: f64, lo: f64, hi: f64| v[k].as_f64().unwrap_or(d).clamp(lo, hi);
        AfterBurst {
            mode: v["mode"].as_str().and_then(AfterMode::from_id).unwrap_or(AfterMode::Video),
            stack_keep_percent: num("stack_keep_percent", 25.0, 1.0, 100.0),
            raw_keep_percent: num("raw_keep_percent", 25.0, 1.0, 100.0),
            margin: num("margin", 2.0, 1.0, 5.0),
        }
    }
    fn to_json(&self) -> Value {
        json!({ "mode": self.mode.id(), "stack_keep_percent": self.stack_keep_percent, "raw_keep_percent": self.raw_keep_percent, "margin": self.margin })
    }
}

/// The burst being captured has been planned for the stacker.
#[derive(Copy, Clone, Debug)]
struct CurStack {
    deferred: bool,
    mode: AfterMode,
}

/// A burst handed to the stacker that has not been answered yet.
struct StackJob {
    sink_no: u64,
    burst_id: u64,
    first: u64,
    count: usize,
    stem: PathBuf,
    rel: String,
    mode: AfterMode,
    deferred: bool,
    sent: Instant,
    est_s: f64,
}

/// Whether the camera will overwrite a burst before its stack is done. `budget` frames can still be captured before the burst's first
/// frame goes; the next burst will capture `next_frames`; the stack needs `remaining_s` more seconds, planned `margin` times over.
/// Safe if the next burst leaves the frames alone, or the stack is done before the camera gets there, with the margin.
fn wrap_unsafe(budget: f64, next_frames: f64, remaining_s: f64, margin: f64, fps: f64) -> bool {
    if next_frames <= budget {
        return false;
    }
    remaining_s * margin > budget / fps.max(1.0)
}

impl Engine {
    fn next_sink_no(&mut self) -> u64 {
        self.sink_counter += 1;
        self.sink_counter
    }

    /// Something the user must see, and that stays until they dismiss it. Also an event in the log.
    fn notice(&mut self, level: &'static str, text: String) {
        self.notice_id += 1;
        self.event("notice", json!({ "level": level, "text": text }));
        log::warn!("{text}");
        let at = chrono::Local::now().format("%H:%M:%S").to_string();
        self.status.notices.push(crate::status::Notice { id: self.notice_id, level, text, at, acked: false });
        let n = self.status.notices.len();
        if n > 50 {
            self.status.notices.drain(..n - 50);
        }
    }

    fn require_after_burst_ready(&self) -> Result<(), String> {
        if self.after.mode.defers() && !self.bridge.connected() {
            return Err(NO_STACKER.into());
        }
        Ok(())
    }

    fn sink_by_no(&self, no: u64) -> Option<&FrameSink> {
        match &self.job {
            Job::Capture(j) if j.sink_no == no => Some(&j.sink),
            _ => self.draining.as_ref().filter(|_| self.draining_no == no),
        }
    }

    /// Decides what happens to the burst that is about to start: handed to the stacker (and whether its video waits for the result) or
    /// plain video. Whenever it is not what was asked for, says so. Returns whether the burst's frames are deferred.
    fn plan_burst(&mut self, job: &mut CaptureJob) -> bool {
        job.cur = None;
        let mode = self.after.mode;
        if mode == AfterMode::Video {
            return false;
        }
        let fps = self.status.camera.as_ref().map_or(30.0, |c| c.fps.max(1.0));
        let expected = job.burst_frames.map(|f| f as f64).or(job.burst_seconds.map(|s| s * fps)).unwrap_or(0.0);
        let mut why: Option<String> = None;
        if !self.bridge.connected() {
            why = Some("the stacker is not connected".into());
        } else if expected + 8.0 > self.buffer_cap as f64 {
            why = Some(format!("a burst of about {expected:.0} frames does not fit in the RAM buffer ({} frames): raise the buffer in More > RAM buffer", self.buffer_cap));
        }
        if why.is_none() {
            self.guard_outstanding(&job.sink, job.sink_no, expected, fps);
        }
        match why {
            None => {
                job.cur = Some(CurStack { deferred: mode.defers(), mode });
                mode.defers()
            }
            Some(w) => {
                if !job.warned_no_stacker {
                    job.warned_no_stacker = true;
                    self.notice("warning", format!("Bursts are being recorded as video and not stacked from memory: {w}."));
                }
                false
            }
        }
    }

    /// Before a burst starts: any earlier burst still waiting for its stack that this one would overwrite before the stack is done is
    /// written as video instead (the stacker keeps going; the stack is still made if the frames survive).
    fn guard_outstanding(&mut self, sink: &FrameSink, sink_no: u64, next_frames: f64, fps: f64) {
        let Some(ring) = sink.ring() else { return };
        let margin = self.after.margin;
        let mut late: Vec<u64> = vec![];
        for (no, j) in self.stack_jobs.iter().filter(|(_, j)| j.sink_no == sink_no && j.deferred) {
            let remaining = (j.est_s - j.sent.elapsed().as_secs_f64()).max(0.25 * j.est_s);
            if wrap_unsafe(ring.frames_until_overwritten(j.first) as f64, next_frames, remaining, margin, fps) {
                late.push(*no);
            }
        }
        for no in late {
            self.fallback(no, "it would not be finished before the next burst overwrote it", Some(sink));
        }
    }

    /// A burst that was waiting for its stack is written as video instead.
    fn fallback(&mut self, no: u64, why: &str, sink: Option<&FrameSink>) {
        let Some(j) = self.stack_jobs.remove(&no) else { return };
        if j.deferred {
            match sink.or_else(|| self.sink_by_no(j.sink_no)) {
                Some(s) => s.resolve(j.burst_id, Resolution::WriteAll),
                None => {}
            }
        }
        let what = if j.deferred { "Its video is being written instead." } else { "Its video is safe." };
        self.notice("warning", format!("The stack of {} was not made: {why}. {what}", j.rel));
    }

    /// The burst has ended: hand it to the stacker.
    fn submit_stack(&mut self, job: &CaptureJob, id: u64, first: u64, last: u64, stem: &std::path::Path, cur: CurStack) {
        let count = (last + 1).saturating_sub(first) as usize;
        let rel_name = format!("{}.ser", rel(&self.data_dir(), stem));
        let ctl = |c: &str| self.status.camera.as_ref().and_then(|cam| cam.controls.iter().find(|x| x.id == c)).map(|x| x.value);
        let (exposure, gain) = (ctl(CTRL_EXPOSURE_US), ctl("gain"));
        self.job_no += 1;
        let no = self.job_no;
        let est_s = count as f64 * self.stack_s_per_frame;
        self.stack_jobs.insert(no, StackJob { sink_no: job.sink_no, burst_id: id, first, count, stem: stem.to_path_buf(), rel: rel_name.clone(), mode: cur.mode, deferred: cur.deferred, sent: Instant::now(), est_s });
        let sent = count >= 4
            && self.bridge.connected()
            && (self.ring_sent == Some(job.sink_no) || job.sink.ring_fd().is_some_and(|fd| {
                use std::os::fd::AsRawFd;
                let ok = self.bridge.send_ring(job.sink_no, fd.as_raw_fd());
                if ok {
                    self.ring_sent = Some(job.sink_no);
                }
                ok
            }))
            && self.bridge.send_burst(&json!({ "t": "burst", "ring": job.sink_no, "id": no, "first": first, "count": count, "stem": rel_name,
                "keep_percent": self.after.stack_keep_percent, "exposure_us": exposure, "gain": gain,
                "disc": if job.disc_track.is_empty() { Value::Null } else { json!(job.disc_track) } }));
        if !sent {
            self.fallback(no, if count < 4 { "the burst has too few frames" } else { "the stacker could not be reached" }, Some(&job.sink));
        }
    }

    /// Answers from the stacker, a lost connection, and closing a recording that was waiting for its stacks.
    fn poll_stack(&mut self) {
        for ev in self.bridge.drain() {
            match ev {
                BridgeEvent::Connected => {
                    self.ring_sent = None;
                    self.event("stacker_connected", json!({}));
                }
                BridgeEvent::Disconnected => {
                    self.ring_sent = None;
                    self.event("stacker_disconnected", json!({ "waiting": self.stack_jobs.len() }));
                    let nos: Vec<u64> = self.stack_jobs.keys().copied().collect();
                    for no in nos {
                        self.fallback(no, "the stacker stopped or lost its connection", None);
                    }
                    if self.after.mode != AfterMode::Video && !matches!(self.job, Job::Idle) {
                        self.notice("warning", "The stacker stopped during a recording: the rest of it is recorded as video.".into());
                    }
                }
                BridgeEvent::Done(v) => self.stack_done(v),
            }
        }
        if self.close_when_stacked && self.stack_jobs.values().all(|j| j.sink_no != self.draining_no) {
            self.close_when_stacked = false;
            if let Some(d) = &self.draining {
                d.close();
            }
        }
    }

    fn stack_done(&mut self, v: Value) {
        let Some(j) = v["id"].as_u64().and_then(|no| self.stack_jobs.remove(&no)) else { return };
        let ok = v["ok"].as_bool() == Some(true);
        let resolution = if !j.deferred {
            None
        } else if !ok {
            Some(Resolution::WriteAll)
        } else if j.mode == AfterMode::StackOnly {
            Some(Resolution::SkipRaw)
        } else {
            let scores: Vec<f64> = v["scores"].as_array().map(|a| a.iter().filter_map(|x| x.as_f64()).collect()).unwrap_or_default();
            if scores.len() != j.count {
                Some(Resolution::WriteAll)
            } else {
                let keep = ((self.after.raw_keep_percent / 100.0 * j.count as f64).ceil() as usize).clamp(1, j.count);
                let mut order: Vec<usize> = (0..j.count).collect();
                order.sort_by(|&a, &b| scores[b].partial_cmp(&scores[a]).unwrap_or(std::cmp::Ordering::Equal));
                let mut best: Vec<u32> = order.into_iter().take(keep).map(|i| i as u32).collect();
                best.sort_unstable();
                Some(Resolution::WriteSelected(best))
            }
        };
        if let Some(r) = resolution {
            if let Some(s) = self.sink_by_no(j.sink_no) {
                s.resolve(j.burst_id, r);
            }
        }
        if ok {
            let secs = v["seconds"].as_f64().unwrap_or(0.0);
            if j.count > 0 && secs > 0.0 {
                self.stack_s_per_frame = (0.5 * self.stack_s_per_frame + 0.5 * secs / j.count as f64).clamp(0.005, 2.0);
            }
            self.stacked_count += 1;
            let output = v["output"].as_str().unwrap_or("").to_string();
            self.last_stack_note = Some(format!("{output} ({} of {} frames, {secs:.1} s)", v["kept"], v["frames"]));
            self.event("stack_done", json!({ "burst": j.rel, "output": output, "seconds": secs, "frames": j.count, "mode": j.mode.id() }));
            let side = j.stem.with_extension("json");
            if let Some(mut sc) = std::fs::read_to_string(&side).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
                sc["details"]["stack"] = json!({ "output": output, "kept": v["kept"], "seconds": secs, "video": match j.mode { AfterMode::StackOnly => "not written", AfterMode::BestFrames => "best frames only", _ => "written" } });
                self.alog.replace(side, serde_json::to_string_pretty(&sc).unwrap_or_default());
            }
        } else {
            let err = v["error"].as_str().unwrap_or("unknown error").to_string();
            let what = if j.deferred { "Its video is being written instead." } else { "Its video is safe." };
            self.notice("warning", format!("The stack of {} was not made: {err}. {what}", j.rel));
        }
    }
}

#[cfg(test)]
mod wrap_tests {
    use super::wrap_unsafe;

    #[test]
    fn a_burst_is_only_given_up_on_when_the_camera_would_get_to_it_before_the_stack_is_done() {
        // the next burst fits in what is left: never a problem, however slow the stack
        assert!(!wrap_unsafe(200.0, 100.0, 60.0, 2.0, 57.0));
        // it does not fit, and the stack (5 s, margin 2 = 10 s) needs longer than the 50 frames at 57 fps (0.9 s) that are left
        assert!(wrap_unsafe(50.0, 100.0, 5.0, 2.0, 57.0));
        // the same stack has time when 600 frames (10.5 s) are left
        assert!(!wrap_unsafe(600.0, 700.0, 5.0, 2.0, 57.0));
        // the margin decides the borderline: 5 s x 2 = 10 s against 10.5 s is fine, against a margin of 3 it is not
        assert!(wrap_unsafe(600.0, 700.0, 5.0, 3.0, 57.0));
        // nothing left at all
        assert!(wrap_unsafe(0.0, 10.0, 0.1, 1.0, 57.0));
    }
}

/// The tracking rate that follows a target: solar for the Sun, lunar for the Moon, sidereal for a planet.
fn rate_of_body(b: Option<rustflare_tracking::bodies::Body>) -> TrackingRate {
    match b { None => TrackingRate::Solar, Some(rustflare_tracking::bodies::Body::Moon) => TrackingRate::Lunar, Some(_) => TrackingRate::Sidereal }
}

fn rate_word(r: TrackingRate) -> &'static str {
    match r { TrackingRate::Solar => "solar", TrackingRate::Lunar => "lunar", TrackingRate::Sidereal => "sidereal" }
}

/// The smallest believable step of the one model of where the target is, in pixels: nothing for the Sun and the Moon (their gates
/// scale with a radius of hundreds of pixels), a floor for a planet a few tens of pixels across that the seeing moves by several.
fn model_gate_floor(profile: &str) -> f64 {
    if profile == "planet" { 8.0 } else { 0.0 }
}

/// Looks for the target in a frame, by profile: the disc and limb detector for the Sun and the Moon, the brightest small blob for a
/// planet (the Sun detector on a planet frame returns a confident phantom disc: see `target_tests`), nothing for an unknown target.
fn detect_target(profile: &str, frame: &Frame, hint: Option<f64>, prev: Option<&SunLocation>) -> Option<SunLocation> {
    match profile {
        "other" => None,
        "planet" => locate_planet(frame, prev.map(|p| (p.pos, p.radius))),
        _ => locate_sun(frame, hint),
    }
}
