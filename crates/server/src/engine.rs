//! The engine owns every piece of hardware and all capture state on one thread.
//! HTTP handlers talk to it with [`Command`]s and read published [`Status`] (ADR 0004).

use crate::preview;
use crate::storage::Storage;
use crate::status::*;
use rustflare_camera as cams;
use rustflare_capture::{write_png, BurstSpec, FlatAccumulator, FlatDecision, FlatTarget, Format, FrameSink, SerWriter};
use rustflare_core::camera::CTRL_EXPOSURE_US;
use rustflare_core::sim::SimSky;
use rustflare_core::{Axis, Camera, CameraDriver, Error, Frame, Mount, MountState, ParkState, RadPerSec, SlewSpeed, TrackingRate, Vec2, SIDEREAL_RATE};
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
    ConnectMount { driver: String, device: Option<String> },
    DisconnectMount,
    MountTracking { enabled: bool },
    MountSlew { axis: Axis, speed: f64 },
    MountStop,
    /// Moves the mount to its home (zero) position; the API asks for explicit confirmation.
    MountHome,
    StartTracking { mode: String, x: Option<f64>, y: Option<f64>, radius: Option<f64> },
    StopTracking,
    Calibrate,
    /// `center`: steer the target to the frame centre instead of holding the current position.
    StartGuiding { center: bool },
    StopGuiding,
    CaptureSingle { name: Option<String> },
    /// One burst (`interval_s` = None) or a schedule of bursts, recorded through the RAM buffer.
    /// `frames` / `seconds`: when a burst ends. `interval_s`: start-to-start spacing. `bursts`: None = until stopped.
    CaptureBursts { name: Option<String>, frames: Option<u32>, seconds: Option<f64>, interval_s: Option<f64>, bursts: Option<u32>, format: Option<String> },
    SetBuffer { mb: u64 },
    /// Reconnect the last camera and mount automatically at startup (and after a camera error).
    SetStartup { enabled: bool },
    CaptureFlats { name: Option<String>, frames: u32, level: Option<f64> },
    CaptureStop,
    SetOptics { focal_length_mm: Option<f64>, focal_multiplier: f64, pixel_size_um: Option<f64>, binning: u32 },
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
struct CaptureJob {
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
    Single { stem: PathBuf },
    Capture(Box<CaptureJob>),
    Flats(Box<Flats>),
}

pub struct Engine {
    drivers: Vec<Box<dyn CameraDriver>>,
    sky: SimSky,
    camera: Option<Box<dyn Camera>>,
    mount: Option<Box<dyn Mount>>,
    mount_tracking: bool,
    /// What the mount itself last reported (it can be changed from outside: its buttons, another app).
    mount_state: MountState,
    last_mount_poll: Instant,
    mount_poll_failures: u32,
    tracker: Option<Tracker>,
    lost: bool,
    guide: GuideState,
    calibration: Option<Calibration>,
    job: Job,
    /// Sink of a finished/aborted capture that is still writing its buffered frames to disk.
    draining: Option<FrameSink>,
    buffer_mb: u64,
    buffer_cap: usize,
    /// Real write speed of the most recent finished capture (MB/s), kept after the writer is gone.
    last_disk_mb_s: f64,
    last_analysis: Instant,
    last_mean: f64,
    last_frame: Option<Frame>,
    storage: Arc<Storage>,
    optics: OpticsSettings,
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
    /// Recent radii measured from the limb when the optics' radius did not fit the data (see `fit_limb_auto`).
    radius_samples: std::collections::VecDeque<f64>,
    /// Median of those, used as the radius hint once it is stable: it fixes the radius-centre degeneracy of short arcs.
    measured_radius: Option<f64>,
    status: Status,
    status_tx: watch::Sender<Status>,
    preview_tx: SyncSender<Frame>,
    last_preview: Instant,
    last_publish: Instant,
    last_temp: Instant,
    fps_ema: f64,
    last_frame_at: Option<Instant>,
}

/// Starts the engine thread.
pub fn spawn(storage: Arc<Storage>, stretch: Arc<std::sync::atomic::AtomicBool>) -> (Handle, std::thread::JoinHandle<()>) {
    let (tx, rx) = mpsc::channel();
    let (status_tx, status_rx) = watch::channel(Status::default());
    let (jpeg_tx, jpeg_rx) = watch::channel(None);
    let sky = SimSky::new();
    let engine = Engine {
        drivers: cams::drivers(sky.clone()),
        sky,
        camera: None,
        mount: None,
        mount_tracking: false,
        mount_state: MountState::default(),
        last_mount_poll: Instant::now(),
        mount_poll_failures: 0,
        tracker: None,
        lost: false,
        guide: GuideState::Off,
        calibration: None,
        job: Job::Idle,
        draining: None,
        buffer_mb: crate::config::read(storage.config_file()).get("capture").and_then(|c| c["buffer_mb"].as_u64()).unwrap_or(1024).clamp(64, 8192),
        buffer_cap: 0,
        last_disk_mb_s: 0.0,
        last_analysis: Instant::now() - Duration::from_secs(1),
        last_mean: 0.0,
        last_frame: None,
        optics: rustflare_config_optics(&storage),
        startup: crate::config::read(storage.config_file()).get("startup").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default(),
        camera_controls: crate::config::read(storage.config_file()).get("camera_controls").and_then(|v| v.as_object().cloned()).unwrap_or_default(),
        controls_dirty: None,
        auto_until: Some(Instant::now() + Duration::from_secs(120)),
        last_auto_try: Instant::now() - Duration::from_secs(10),
        camera_pixel_um: None,
        expected_radius: None,
        radius_samples: Default::default(),
        measured_radius: None,
        storage,
        status: Status::default(),
        status_tx,
        preview_tx: preview::spawn(jpeg_tx, stretch),
        last_preview: Instant::now(),
        last_publish: Instant::now(),
        last_temp: Instant::now(),
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

impl Engine {
    fn run(mut self, rx: Receiver<Request>) {
        loop {
            self.poll_sink();
            self.auto_connect_tick();
            self.poll_mount();
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
                Ok(frame) => self.on_frame(frame),
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
        if res.is_ok() {
            self.status.error = None;
        }
        self.publish();
        let _ = r.reply.send(res);
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
        self.expected_radius = effective.zip(pixel).map(|(f, p)| solar::expected_radius_px(f, p, bin, day));
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
            suggested_effective_focal_mm: self.measured_radius.zip(pixel).map(|(r, p)| solar::focal_from_radius(r, p, bin, day)),
            suggested_multiplier: self.measured_radius.zip(pixel).zip(self.optics.focal_length_mm).map(|((r, p), f)| solar::focal_from_radius(r, p, bin, day) / f),
        };
    }

    fn publish(&mut self) {
        self.refresh_optics();
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
        let c = &mut self.status.capture;
        c.buffer_queued = queued;
        c.buffer_capacity = self.buffer_cap;
        c.buffer_mb = self.buffer_mb;
        c.ram_available_mb = mem_available_mb();
        c.disk_mb_s = if rate > 0.0 { rate / 1e6 } else { self.last_disk_mb_s };
        if matches!(self.job, Job::Idle) && self.draining.is_some() {
            c.state = "flushing";
            c.phase = if queued > 0 { format!("writing {queued} buffered frames to disk") } else { "flushing to the storage medium (safe to unplug when done)".into() };
        }
        self.status.seq += 1;
        self.last_publish = Instant::now();
        let ms = self.mount_state;
        self.status.mount = self.mount.as_ref().map(|m| MountStatus { info: m.info(), tracking: self.mount_tracking, slewing: ms.slewing, at_home: ms.at_home, park: ms.park, alt_az: ms.alt_az });
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
            Command::MountTracking { enabled } => {
                if enabled {
                    self.require_not_parked()?;
                }
                let m = self.mount.as_mut().ok_or("no mount connected")?;
                if enabled {
                    m.set_tracking_rate(TrackingRate::Solar).map_err(s)?;
                }
                m.set_tracking(enabled).map_err(s)?;
                self.mount_tracking = enabled;
                if !enabled {
                    self.stop_guiding_quietly();
                }
                Ok(json!({}))
            }
            Command::MountSlew { axis, speed } => {
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
                self.stop_guiding_quietly();
                self.mount.as_mut().ok_or("no mount connected")?.stop().map_err(s)?;
                Ok(json!({}))
            }
            Command::MountHome => {
                self.require_idle()?;
                self.require_not_parked()?;
                self.stop_guiding_quietly();
                self.mount.as_mut().ok_or("no mount connected")?.go_home().map_err(s)?;
                self.mount_tracking = false; // the mount stops tracking to go home; the next poll confirms
                Ok(json!({}))
            }
            Command::StartTracking { mode, x, y, radius } => {
                self.require_not_flats()?;
                let f = self.last_frame.as_ref().ok_or("no frame received yet")?;
                let t = match mode.as_str() {
                    "sun_disc" => Tracker::sun_disc(f),
                    "limb" => Tracker::limb(f, radius.or(self.radius_hint())),
                    "anchor" => Tracker::anchor(f, Vec2::new(x.ok_or("anchor needs x,y")?, y.ok_or("anchor needs x,y")?)),
                    other => return Err(format!("unknown tracking mode {other}")),
                }
                .map_err(s)?;
                self.stop_guiding_quietly();
                let p = t.position();
                self.status.tracking = Some(TrackingStatus { mode: t.mode(), x: p.pos.x, y: p.pos.y, radius: p.radius, lost: false });
                self.tracker = Some(t);
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
                self.guide = GuideState::Guiding(Guider::new(pos, self.calibration.unwrap(), GuideParams::default()));
                Ok(json!({}))
            }
            Command::StopGuiding => {
                self.stop_guiding_quietly();
                Ok(json!({}))
            }
            Command::CaptureSingle { name } => {
                self.require_capture_ready()?;
                self.job = Job::Single { stem: self.new_stem("frame", name.as_deref()) };
                self.set_capture("single", "waiting for frame", None);
                Ok(json!({}))
            }
            Command::CaptureBursts { name, frames, seconds, interval_s, bursts, format } => {
                self.require_capture_ready()?;
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
                    Some(o) => return Err(format!("unknown format {o} (use ser or png)")),
                };
                let cam = self.status.camera.as_ref().ok_or("no camera open")?;
                if cam.width == 0 {
                    return Err("no frame received yet".into());
                }
                let frame_bytes = cam.width as u64 * cam.height as u64 * (cam.bits as u64 / 8).max(1);
                let cap = self.buffer_capacity(frame_bytes);
                self.buffer_cap = cap;
                let kind = if interval.is_some() { "burst" } else { "seq" };
                self.job = Job::Capture(Box::new(CaptureJob {
                    name: name.unwrap_or_default(),
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
                    sink: FrameSink::new(cap),
                }));
                self.set_capture(if interval.is_some() { "bursts" } else { "sequence" }, "starting", frames);
                self.status.capture.bursts_total = total_bursts;
                Ok(json!({ "buffer_frames": cap, "fits_in_buffer": frames.map_or(true, |f| f as usize <= cap) }))
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
                if focal_length_mm.is_some_and(|f| !(10.0..=20000.0).contains(&f)) {
                    return Err("focal length must be between 10 and 20000 mm".into());
                }
                if pixel_size_um.is_some_and(|p| !(0.5..=50.0).contains(&p)) {
                    return Err("pixel size must be between 0.5 and 50 µm".into());
                }
                if !(0.2..=10.0).contains(&focal_multiplier) {
                    return Err("Barlow/reducer factor must be between 0.2 and 10 (e.g. 2 for a 2x Barlow, 0.63 for a reducer)".into());
                }
                if !(1..=8).contains(&binning) {
                    return Err("binning must be 1 to 8".into());
                }
                self.optics = OpticsSettings { focal_length_mm, focal_multiplier, pixel_size_um, binning };
                self.radius_samples.clear();
                self.measured_radius = None;
                crate::config::set(self.storage.config_file(), "optics", serde_json::to_value(&self.optics).unwrap_or_default())?;
                Ok(json!({}))
            }
            Command::CaptureStop => {
                if matches!(self.job, Job::Idle) {
                    return Err("nothing is being captured".into());
                }
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
        if self.mount.is_none() || self.last_mount_poll.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.last_mount_poll = Instant::now();
        match self.mount.as_mut().map(|m| m.state()) {
            Some(Ok(st)) => {
                self.mount_poll_failures = 0;
                if let Some(t) = st.tracking {
                    if t != self.mount_tracking {
                        log::info!("mount tracking is now {} (changed outside RustFlare)", if t { "on" } else { "off" });
                        if !t && !matches!(self.guide, GuideState::Off) {
                            self.stop_guiding_quietly();
                            self.status.error = Some("the mount stopped tracking (changed on the mount itself): guiding stopped".into());
                        }
                        self.mount_tracking = t;
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
                    self.status.error = Some(format!("lost contact with the mount: {e}"));
                }
            }
            None => {}
        }
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
    fn require_not_flats(&self) -> Result<(), String> {
        if matches!(self.job, Job::Flats(_)) { Err("not while capturing flats".into()) } else { Ok(()) }
    }
    fn data_dir(&self) -> PathBuf {
        self.storage.dir()
    }
    fn require_capture_ready(&self) -> Result<(), String> {
        if self.camera.is_none() {
            return Err("no camera open".into());
        }
        self.require_idle()?;
        self.storage.ensure_ready().map(|_| ())
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
        self.status.camera = Some(CameraStatus { driver: info.driver, id: info.id, name: info.name, controls, ..Default::default() });
        self.camera = Some(cam);
        Ok(())
    }

    fn connect_mount(&mut self, driver: &str, device: Option<&str>) -> Result<(), String> {
        self.disconnect_mount();
        let m = rustflare_mount::connect(driver, device, &self.sky).map_err(s)?;
        self.mount = Some(m);
        // Connecting never changes the mount's tracking; read back what it is actually doing.
        self.mount_tracking = false;
        self.mount_state = MountState::default();
        if let Some(Ok(st)) = self.mount.as_mut().map(|m| m.state()) {
            self.mount_tracking = st.tracking.unwrap_or(false);
            self.mount_state = st;
        }
        Ok(())
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
        if self.mount.is_none() {
            if let Some((driver, device)) = self.startup.mount.clone() {
                match self.connect_mount(&driver, device.as_deref()) {
                    Ok(()) => { log::info!("reconnected mount {driver} {device:?} (tracking stays off)"); changed = true; }
                    Err(e) => log::debug!("auto mount {driver}: {e}"),
                }
            }
        }
        let done = (self.camera.is_some() || self.startup.camera.is_none()) && (self.mount.is_some() || self.startup.mount.is_none());
        if done {
            self.auto_until = None;
        }
        if changed {
            self.publish();
        }
    }

    fn close_camera(&mut self) {
        self.camera = None;
        self.camera_pixel_um = None;
        self.status.camera = None;
        self.last_frame = None;
        self.tracker = None;
        self.status.tracking = None;
        self.stop_guiding_quietly();
    }

    fn disconnect_mount(&mut self) {
        self.stop_guiding_quietly();
        self.mount = None;
        self.mount_tracking = false;
        self.mount_state = MountState::default();
        self.calibration = None;
    }

    fn stop_guiding_quietly(&mut self) {
        let was_active = !matches!(self.guide, GuideState::Off);
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

    fn on_frame(&mut self, frame: Frame) {
        let now = Instant::now();
        if let Some(prev) = self.last_frame_at.replace(now) {
            let inst = 1.0 / now.duration_since(prev).as_secs_f64().max(1e-3);
            self.fps_ema = if self.fps_ema == 0.0 { inst } else { 0.9 * self.fps_ema + 0.1 * inst };
        }
        // Analysis (statistics, histogram, Sun location, tracking) runs at ~10 Hz regardless of the camera's
        // frame rate: it is far more than guiding needs, and it keeps fast cameras free for recording.
        let mut mean = self.last_mean;
        if now.duration_since(self.last_analysis) >= Duration::from_millis(100) {
            self.last_analysis = now;
            mean = frame.mean_fraction();
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
                c.saturated = frame.saturated_fraction();
                c.histogram = frame.histogram(64);
                if refresh_temp {
                    c.temperature_c = temp;
                }
            }
            let sun = locate_sun(&frame, self.radius_hint());
            self.learn_radius(sun.as_ref());
            self.status.sun = sun;
            self.step_tracking(&frame, now);
            self.last_frame = Some(frame.clone());
        }
        if let Some(c) = self.status.camera.as_mut() {
            c.fps = self.fps_ema;
        }
        if self.last_preview.elapsed() >= Duration::from_millis(66) {
            self.last_preview = now;
            let _ = self.preview_tx.try_send(frame.clone());
        }
        // The frame is moved (not copied) into the recording path.
        self.step_job(frame, mean);
        if self.last_publish.elapsed() >= Duration::from_millis(150) {
            self.publish();
        }
    }

    fn step_tracking(&mut self, frame: &Frame, now: Instant) {
        let Some(t) = self.tracker.as_mut() else { return };
        match t.update(frame) {
            Ok(p) => {
                self.lost = false;
                self.status.tracking = Some(TrackingStatus { mode: t.mode(), x: p.pos.x, y: p.pos.y, radius: p.radius, lost: false });
                self.step_guide(now, p.pos);
            }
            Err(e) => {
                let first = !self.lost;
                self.lost = true;
                if let Some(ts) = self.status.tracking.as_mut() {
                    ts.lost = true;
                }
                if first {
                    log::warn!("tracking lost: {e}");
                    self.status.error = Some(format!("tracking lost: {e}"));
                    self.stop_guiding_quietly();
                }
            }
        }
    }

    fn step_guide(&mut self, now: Instant, pos: Vec2) {
        let mut cmd = None;
        let mut failure = None;
        match &mut self.guide {
            GuideState::Off => {}
            GuideState::Guiding(g) => {
                cmd = g.step(now, pos);
                // show the averaged offset the guider acts on (the instantaneous one is mostly seeing)
                self.status.guiding.error_px = Some(g.filtered_error().map_or((g.target - pos).len(), |e| e.len()));
            }
            GuideState::Calibrating(c) => match c.step(now, pos) {
                CalAction::None => {}
                CalAction::Guide(g) => cmd = Some(g),
                CalAction::Done(cal) => {
                    self.calibration = Some(cal);
                    self.guide = GuideState::Off;
                    cmd = Some(GuideCommand::STOP);
                }
                CalAction::Failed(msg) => {
                    failure = Some(format!("calibration failed: {msg}"));
                    self.guide = GuideState::Off;
                    cmd = Some(GuideCommand::STOP);
                }
            },
        }
        if let Some(c) = cmd {
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
        let job = std::mem::replace(&mut self.job, Job::Idle);
        match job {
            Job::Idle | Job::Single { .. } => {}
            Job::Capture(job) => {
                let CaptureJob { sink, phase, kind, name, burst_no, .. } = *job;
                if let CapPhase::Capturing { stem, taken, started } = phase {
                    sink.end();
                    let sc = self.sidecar(if kind == "burst" { "burst" } else { "sequence" }, json!({ "name": name, "burst": burst_no, "frames": taken, "duration_s": started.elapsed().as_secs_f64(), "stopped_early": why }));
                    self.write_sidecar(&stem, &sc);
                }
                sink.close();
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
            Job::Single { stem } => {
                let path = stem.with_extension("png");
                let r = std::fs::create_dir_all(path.parent().unwrap()).map_err(s).and_then(|_| write_png(&path, &frame).map_err(s));
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
            Job::Flats(f) => {
                let m = frame.mean_fraction(); // flats need a fresh level for every frame
                let _ = mean;
                self.step_flats(f, &frame, m)
            }
        };
    }

    fn step_capture(&mut self, mut job: Box<CaptureJob>, frame: Frame) -> Job {
        let now = Instant::now();
        let total_txt = job.total_bursts.map(|t| format!("/{t}")).unwrap_or_default();
        if let CapPhase::Waiting { next_at } = job.phase {
            if now < next_at {
                self.status.capture.next_in_s = Some((next_at - now).as_secs_f64());
                self.status.capture.phase = format!("waiting {:.0} s for burst {}{total_txt}", (next_at - now).as_secs_f64().ceil(), job.burst_no + 1);
                return Job::Capture(job);
            }
            job.burst_no += 1;
            let mut stem = self.new_stem(job.kind, Some(&job.name));
            if job.kind == "burst" {
                stem = stem.with_file_name(format!("{}_b{:03}", stem.file_name().unwrap().to_string_lossy(), job.burst_no));
            }
            let instrument = self.status.camera.as_ref().map(|c| c.name.clone()).unwrap_or_default();
            job.sink.begin(BurstSpec { stem: stem.clone(), format: job.format, instrument, max_file_bytes: job.max_file });
            self.status.capture.burst = Some(job.burst_no);
            self.status.capture.next_in_s = None;
            self.status.capture.done = 0;
            self.status.capture.file = Some(rel(&self.data_dir(), &stem));
            job.phase = CapPhase::Capturing { stem, started: now, taken: 0 };
        }
        let CapPhase::Capturing { stem, started, taken } = &mut job.phase else { unreachable!() };
        if !job.sink.push(frame) {
            return self.fail_capture(job, "the disk writer stopped unexpectedly".into());
        }
        *taken += 1;
        self.status.capture.done = *taken;
        self.status.capture.phase = format!("burst {}{total_txt}: capturing", job.burst_no);

        let mut stop: Option<String> = job.sink.error().map(|e| format!("writing failed: {e}"));
        if stop.is_none() && job.last_check.elapsed() >= Duration::from_secs(2) {
            job.last_check = now;
            stop = self.storage.running_problem();
        }
        let burst_done = stop.is_some()
            || job.burst_frames.is_some_and(|n| *taken >= n)
            || job.burst_seconds.is_some_and(|sec| started.elapsed().as_secs_f64() >= sec);
        if !burst_done {
            return Job::Capture(job);
        }

        let (stem, taken, started) = (stem.clone(), *taken, *started);
        job.sink.end();
        let sc = self.sidecar(
            if job.kind == "burst" { "burst" } else { "sequence" },
            json!({ "name": job.name, "burst": job.burst_no, "of": job.total_bursts, "frames": taken,
                    "format": if job.format == Format::Png { "png" } else { "ser" },
                    "duration_s": started.elapsed().as_secs_f64(), "stopped_early": stop }),
        );
        self.write_sidecar(&stem, &sc);
        if let Some(why) = stop {
            self.status.error = Some(format!("recording stopped: {why} ({taken} frames in this burst)"));
            return self.fail_capture_keep(job);
        }
        let last = job.interval.is_none() || job.total_bursts.is_some_and(|t| job.burst_no >= t);
        if last {
            let CaptureJob { sink, .. } = *job;
            sink.close();
            self.draining = Some(sink);
            self.finish_capture(None, "captured; writing buffered frames to disk");
            return Job::Idle;
        }
        let next_at = (started + job.interval.unwrap()).max(Instant::now());
        job.phase = CapPhase::Waiting { next_at };
        Job::Capture(job)
    }

    /// Ends a capture job after an error, keeping whatever was already written.
    fn fail_capture(&mut self, job: Box<CaptureJob>, why: String) -> Job {
        self.status.error = Some(why);
        self.fail_capture_keep(job)
    }

    fn fail_capture_keep(&mut self, job: Box<CaptureJob>) -> Job {
        let CaptureJob { sink, .. } = *job;
        sink.close();
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
            self.status.capture.last_file = Some(rel(&self.data_dir(), &f.path));
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
                        let r = std::fs::create_dir_all(path.parent().unwrap()).map_err(s).and_then(|_| SerWriter::create(&path, frame.width, frame.height, frame.format, &cam, "Rustflare flats").map_err(s));
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
                let master_res = acc.master().map_err(s).and_then(|m| write_png(&master, &m).map_err(s));
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

fn rel(base: &std::path::Path, p: &std::path::Path) -> String {
    p.strip_prefix(base).unwrap_or(p).to_string_lossy().to_string()
}
