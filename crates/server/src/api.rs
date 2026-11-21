//! HTTP/WebSocket API. Handlers are thin: they translate JSON to [`Command`]s.

use crate::engine::{Command, Handle};
use axum::body::Body;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use rustflare_core::Axis;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use tower::ServiceExt;
use std::time::Duration;
use tower_http::services::{ServeDir, ServeFile};

#[derive(Clone)]
pub struct AppState {
    pub engine: Handle,
    pub storage: std::sync::Arc<crate::storage::Storage>,
    /// Preview auto-stretch (applies to everyone watching).
    pub stretch: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Counts live viewers so the preview is only produced while someone watches.
    pub demand: std::sync::Arc<crate::preview::Demand>,
}

pub fn router(state: AppState, web_dir: PathBuf) -> Router {
    let index = web_dir.join("index.html");
    let spa = ServeDir::new(&web_dir).not_found_service(ServeFile::new(index));
    Router::new()
        .route("/api/status", get(status))
        .route("/api/ws", get(ws))
        .route("/api/cameras", get(|s: State<AppState>| run(s, Command::ListCameras)))
        .route("/api/camera/open", post(camera_open))
        .route("/api/camera/close", post(|s| run(s, Command::CloseCamera)))
        .route("/api/camera/control", put(camera_control))
        .route("/api/mount/ports", get(mount_ports))
        .route("/api/mount/scan", get(mount_scan))
        .route("/api/mount/drivers", get(|| async { Json(json!(rustflare_mount::DRIVERS)) }))
        .route("/api/mount/connect", post(mount_connect))
        .route("/api/mount/disconnect", post(|s| run(s, Command::DisconnectMount)))
        .route("/api/mount/tracking", post(mount_tracking))
        .route("/api/mount/slew", post(mount_slew))
        .route("/api/mount/stop", post(|s| run(s, Command::MountStop)))
        .route("/api/mount/home", post(|s, b| mount_motion(s, b, Command::MountHome)))
        .route("/api/mount/goto-sun", post(goto_sun))
        .route("/api/site", put(site_put))
        .route("/api/tracking/start", post(tracking_start))
        .route("/api/tracking/stop", post(|s| run(s, Command::StopTracking)))
        .route("/api/guiding/calibrate", post(|s| run(s, Command::Calibrate)))
        .route("/api/guiding/start", post(guiding_start))
        .route("/api/guiding/stop", post(|s| run(s, Command::StopGuiding)))
        .route("/api/capture/single", post(capture_single))
        .route("/api/capture/sequence", post(capture_sequence))
        .route("/api/capture/bursts", post(capture_bursts))
        .route("/api/capture/buffer", put(capture_buffer))
        .route("/api/capture/flats", post(capture_flats))
        .route("/api/capture/prominence", post(capture_prominence))
        .route("/api/capture/stop", post(|s| run(s, Command::CaptureStop)))
        .route("/api/files", get(files))
        .route("/stream.mjpg", get(mjpeg))
        .route("/snapshot.jpg", get(snapshot))
        .route("/files/*path", get(download))
        .route("/api/storage", get(storage_get).put(storage_put))
        .route("/api/storage/eject", post(storage_eject))
        .route("/api/flats/applied", put(flats_applied))
        .route("/api/guiding/params", put(guide_params))
        .route("/api/coronagraph", put(coronagraph))
        .route("/api/capture/darks", post(capture_darks))
        .route("/api/darks/applied", put(darks_applied))
        .route("/api/datasheet/:kind", get(datasheet))
        .route("/api/files/stats", get(file_stats))
        .route("/api/files/delete", post(file_delete))
        .route("/api/optics", put(optics_put))
        .route("/api/startup", put(startup_put))
        .route("/api/ui", get(ui_get).put(ui_put))
        .route("/api/preview", get(preview_get).put(preview_put))
        .fallback_service(spa)
        .with_state(state)
}

type Res = Response;

async fn run(State(st): State<AppState>, cmd: Command) -> Res {
    match st.engine.call(cmd).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
    }
}

async fn status(State(st): State<AppState>) -> Json<Value> {
    Json(serde_json::to_value(&*st.engine.status.borrow()).unwrap_or_default())
}

async fn ws(State(st): State<AppState>, up: WebSocketUpgrade) -> Res {
    up.on_upgrade(move |sock| ws_loop(sock, st.engine.status.clone()))
}

async fn ws_loop(mut sock: WebSocket, mut rx: tokio::sync::watch::Receiver<crate::status::Status>) {
    loop {
        let txt = serde_json::to_string(&*rx.borrow_and_update()).unwrap_or_default();
        if sock.send(Message::Text(txt)).await.is_err() {
            return;
        }
        tokio::select! {
            r = rx.changed() => { if r.is_err() { return; } }
            m = sock.recv() => { if matches!(m, None | Some(Err(_)) | Some(Ok(Message::Close(_)))) { return; } }
        }
        // at most ~7 updates/s per client
        tokio::time::sleep(Duration::from_millis(140)).await;
    }
}

#[derive(Deserialize)]
struct OpenCamera { driver: String, id: String }
async fn camera_open(s: State<AppState>, Json(b): Json<OpenCamera>) -> Res {
    run(s, Command::OpenCamera { driver: b.driver, id: b.id }).await
}

#[derive(Deserialize)]
struct Control { id: String, value: f64 }
async fn camera_control(s: State<AppState>, Json(b): Json<Control>) -> Res {
    run(s, Command::SetControl { id: b.id, value: b.value }).await
}

#[derive(Deserialize)]
struct MountConnect { driver: String, device: Option<String>, transport: Option<String> }

/// Maps the request to the device string the engine stores: a serial path as is (USB, the old form), or
/// `ble:<address>` for Bluetooth.
fn connect_device(transport: Option<&str>, device: Option<String>) -> Result<Option<String>, String> {
    let device = device.filter(|d| !d.is_empty());
    match transport.unwrap_or("usb") {
        "usb" | "serial" => Ok(device),
        "ble" => {
            let a = device.ok_or("Bluetooth needs the mount's address (scan first)")?;
            let a = a.strip_prefix(rustflare_mount::BLE_PREFIX).unwrap_or(&a).to_uppercase();
            let ok = a.split(':').count() == 6 && a.split(':').all(|p| p.len() == 2 && p.bytes().all(|b| b.is_ascii_hexdigit()));
            if !ok {
                return Err(format!("not a Bluetooth address: {a:?}"));
            }
            Ok(Some(format!("{}{a}", rustflare_mount::BLE_PREFIX)))
        }
        other => Err(format!("unknown transport {other:?} (usb or ble)")),
    }
}

async fn mount_connect(s: State<AppState>, Json(b): Json<MountConnect>) -> Res {
    match connect_device(b.transport.as_deref(), b.device) {
        Ok(device) => run(s, Command::ConnectMount { driver: b.driver, device }).await,
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
    }
}

/// Nearby Bluetooth mounts (takes about 6 seconds). Does not go through the engine thread.
async fn mount_scan() -> Res {
    static SCANNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if SCANNING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return (StatusCode::CONFLICT, Json(json!({ "error": "a scan is already running" }))).into_response();
    }
    let r = tokio::task::spawn_blocking(|| rustflare_mount::ble::scan(Duration::from_secs(6))).await;
    SCANNING.store(false, std::sync::atomic::Ordering::SeqCst);
    match r {
        Ok(Ok(devs)) => Json(json!(devs)).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e.to_string() }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    }
}

#[derive(Deserialize)]
struct Enabled { enabled: bool }
async fn mount_tracking(s: State<AppState>, Json(b): Json<Enabled>) -> Res {
    run(s, Command::MountTracking { enabled: b.enabled }).await
}

#[derive(Deserialize)]
struct Slew { axis: Axis, /** signed multiples of sidereal; 0 stops */ speed: f64 }
async fn mount_slew(s: State<AppState>, Json(b): Json<Slew>) -> Res {
    run(s, Command::MountSlew { axis: b.axis, speed: b.speed }).await
}

#[derive(Deserialize)]
struct TrackStart { mode: String, x: Option<f64>, y: Option<f64>, radius: Option<f64> }
async fn tracking_start(s: State<AppState>, Json(b): Json<TrackStart>) -> Res {
    run(s, Command::StartTracking { mode: b.mode, x: b.x, y: b.y, radius: b.radius }).await
}

#[derive(Deserialize, Default)]
struct GuideStart { #[serde(default)] center: bool }
async fn guiding_start(s: State<AppState>, body: Option<Json<GuideStart>>) -> Res {
    run(s, Command::StartGuiding { center: body.map(|b| b.0.center).unwrap_or(false) }).await
}

#[derive(Deserialize)]
struct Single { name: Option<String>, format: Option<String> }
async fn capture_single(s: State<AppState>, Json(b): Json<Single>) -> Res {
    run(s, Command::CaptureSingle { name: b.name, format: b.format }).await
}

#[derive(Deserialize)]
struct Sequence { name: Option<String>, frames: Option<u32>, seconds: Option<f64>, format: Option<String>, on_full: Option<String> }
async fn capture_sequence(s: State<AppState>, Json(b): Json<Sequence>) -> Res {
    run(s, Command::CaptureBursts { name: b.name, frames: b.frames, seconds: b.seconds, interval_s: None, bursts: None, format: b.format, on_full: b.on_full }).await
}

#[derive(Deserialize)]
struct Bursts { name: Option<String>, frames: Option<u32>, seconds: Option<f64>, interval_s: f64, bursts: Option<u32>, format: Option<String>, on_full: Option<String> }
async fn capture_bursts(s: State<AppState>, Json(b): Json<Bursts>) -> Res {
    run(s, Command::CaptureBursts { name: b.name, frames: b.frames, seconds: b.seconds, interval_s: Some(b.interval_s), bursts: b.bursts, format: b.format, on_full: b.on_full }).await
}

#[derive(Deserialize)]
struct Buffer { mb: u64 }
async fn capture_buffer(s: State<AppState>, Json(b): Json<Buffer>) -> Res {
    run(s, Command::SetBuffer { mb: b.mb }).await
}

#[derive(Deserialize)]
struct Prominence { name: Option<String>, factor: Option<f64> }
async fn capture_prominence(s: State<AppState>, Json(b): Json<Prominence>) -> Res {
    run(s, Command::CaptureProminence { name: b.name, factor: b.factor.unwrap_or(8.0) }).await
}

#[derive(Deserialize)]
struct Flats { name: Option<String>, frames: u32, level: Option<f64> }
async fn capture_flats(s: State<AppState>, Json(b): Json<Flats>) -> Res {
    run(s, Command::CaptureFlats { name: b.name, frames: b.frames, level: b.level }).await
}

/// Lists capture files (two levels deep: date folder / file).
async fn files(State(st): State<AppState>) -> Json<Value> {
    let mut out = vec![];
    if let Ok(days) = std::fs::read_dir(st.storage.dir()) {
        for day in days.flatten().filter(|d| d.path().is_dir()) {
            let dname = day.file_name().to_string_lossy().to_string();
            if let Ok(fs) = std::fs::read_dir(day.path()) {
                for f in fs.flatten() {
                    if let Ok(md) = f.metadata() {
                        // a PNG burst is a folder of frames: report its frame count and total size
                        let (size, folder_files) = if md.is_dir() {
                            let items: Vec<_> = std::fs::read_dir(f.path()).map(|d| d.flatten().collect()).unwrap_or_default();
                            (items.iter().filter_map(|i| i.metadata().ok()).map(|m| m.len()).sum(), Some(items.len()))
                        } else {
                            (md.len(), None)
                        };
                        out.push(json!({
                            "path": format!("{dname}/{}", f.file_name().to_string_lossy()),
                            "size": size,
                            "folder_files": folder_files,
                            "modified": md.modified().ok().and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()),
                        }));
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| b["path"].as_str().cmp(&a["path"].as_str()));
    Json(json!(out))
}

async fn snapshot(State(st): State<AppState>) -> Res {
    if !st.demand.wanted() {
        st.demand.want_snapshot();
        tokio::time::sleep(Duration::from_millis(250)).await; // let a fresh frame come through
    } else {
        st.demand.want_snapshot();
    }
    match st.engine.preview.borrow().clone() {
        Some(j) => ([(header::CONTENT_TYPE, "image/jpeg"), (header::CACHE_CONTROL, "no-store")], j.to_vec()).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

/// multipart/x-mixed-replace stream; works in a plain <img> tag.
async fn mjpeg(State(st): State<AppState>, axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>) -> Res {
    const B: &str = "rustflareframe";
    // ?fps=N caps this viewer's frame rate (saves Wi-Fi bandwidth)
    let min_gap = q.get("fps").and_then(|v| v.parse::<f64>().ok()).filter(|f| *f >= 0.5).map(|f| Duration::from_secs_f64(1.0 / f.min(30.0)));
    let rx = st.engine.preview.clone();
    let guard = st.demand.viewer();
    let stream = futures_util::stream::unfold((rx, std::time::Instant::now() - Duration::from_secs(10), guard), move |(mut rx, mut last, guard)| async move {
        loop {
            if rx.changed().await.is_err() {
                return None;
            }
            if min_gap.is_some_and(|g| last.elapsed() < g) {
                continue;
            }
            let jpg = rx.borrow_and_update().clone();
            if let Some(j) = jpg {
                last = std::time::Instant::now();
                let mut chunk = format!("--{B}\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n", j.len()).into_bytes();
                chunk.extend_from_slice(&j);
                chunk.extend_from_slice(b"\r\n");
                return Some((Ok::<_, std::convert::Infallible>(chunk), (rx, last, guard)));
            }
        }
    });
    Response::builder()
        .header(header::CONTENT_TYPE, format!("multipart/x-mixed-replace; boundary={B}"))
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from_stream(stream))
        .unwrap()
}

/// Serves a capture file from the *current* storage directory (it can change at run time).
async fn download(State(st): State<AppState>, axum::extract::Path(path): axum::extract::Path<String>, req: axum::extract::Request) -> Res {
    if path.split('/').any(|c| c == ".." || c.is_empty()) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let (mut parts, body) = req.into_parts();
    parts.uri = match format!("/{path}").parse() {
        Ok(u) => u,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    match ServeDir::new(st.storage.dir()).oneshot(axum::http::Request::from_parts(parts, body)).await {
        Ok(r) => r.into_response(),
        Err(e) => match e {},
    }
}

async fn storage_get(State(st): State<AppState>) -> Json<Value> {
    let s = st.storage.clone();
    let v = tokio::task::spawn_blocking(move || json!({ "current": s.info(), "candidates": s.candidates() })).await.unwrap_or_default();
    Json(v)
}

#[derive(Deserialize)]
struct StoragePath { path: String }
async fn storage_put(State(st): State<AppState>, Json(b): Json<StoragePath>) -> Res {
    if st.engine.status.borrow().capture.state != "idle" {
        return (StatusCode::CONFLICT, Json(json!({ "error": "a capture is running" }))).into_response();
    }
    let s = st.storage.clone();
    match tokio::task::spawn_blocking(move || s.select(std::path::Path::new(&b.path))).await {
        Ok(Ok(info)) => Json(json!(info)).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    }
}

const CATALOG: &str = include_str!("../data/catalog.json");

/// Device card data: what the device reports now (`live`) and the vendor catalog entry matching it (`catalog`, or null).
async fn datasheet(State(st): State<AppState>, axum::extract::Path(kind): axum::extract::Path<String>) -> Res {
    let cat: Value = serde_json::from_str(CATALOG).unwrap_or_default();
    let status = st.engine.status.borrow().clone();
    let (live, name, list) = match kind.as_str() {
        "camera" => {
            let Some(c) = &status.camera else { return (StatusCode::NOT_FOUND, Json(json!({ "error": "no camera open" }))).into_response() };
            (json!({ "model": c.name, "width": c.width, "height": c.height, "bits": c.bits, "pixel_um": status.optics.pixel_size_um }), c.name.clone(), "cameras")
        }
        "mount" => {
            let Some(m) = &status.mount else { return (StatusCode::NOT_FOUND, Json(json!({ "error": "no mount connected" }))).into_response() };
            (json!({ "model": m.info, "tracking": m.tracking }), m.info.clone(), "mounts")
        }
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    // the longest matching name wins ("Apollo-M MINI" beats "Apollo-M")
    let lname = name.to_lowercase();
    let entry = cat[list].as_array().and_then(|a| a.iter().filter(|e| e["match"].as_str().is_some_and(|m| lname.contains(&m.to_lowercase()))).max_by_key(|e| e["match"].as_str().map_or(0, str::len))).cloned();
    Json(json!({ "live": live, "catalog": entry })).into_response()
}

#[derive(Deserialize)]
struct GuideParamsBody { window_s: f64, deadband_px: f64, gain: f64, speed: f64, fast_speed: f64, fast_above_px: f64 }
async fn guide_params(s: State<AppState>, Json(b): Json<GuideParamsBody>) -> Res {
    run(s, Command::SetGuideParams { window_s: b.window_s, deadband_px: b.deadband_px, gain: b.gain, speed: b.speed, fast_speed: b.fast_speed, fast_above_px: b.fast_above_px }).await
}

#[derive(Deserialize)]
struct DarksBody { name: Option<String>, frames: Option<u32> }
async fn capture_darks(s: State<AppState>, Json(b): Json<DarksBody>) -> Res {
    run(s, Command::CaptureDarks { name: b.name, frames: b.frames.unwrap_or(30) }).await
}
async fn darks_applied(s: State<AppState>, Json(b): Json<FlatApplied>) -> Res {
    run(s, Command::SetDarkApplied { enabled: b.enabled }).await
}

#[derive(Deserialize)]
struct CoronagraphBody { enabled: bool, scale: Option<f64> }
async fn coronagraph(s: State<AppState>, Json(b): Json<CoronagraphBody>) -> Res {
    run(s, Command::SetCoronagraph { enabled: b.enabled, scale: b.scale.unwrap_or(1.03) }).await
}

#[derive(Deserialize)]
struct FlatApplied { enabled: bool }
async fn flats_applied(s: State<AppState>, Json(b): Json<FlatApplied>) -> Res {
    run(s, Command::SetFlatApplied { enabled: b.enabled }).await
}

#[derive(Deserialize)]
struct PathQuery { path: String }
/// Header, timing and brightness statistics of one SER file, with a "mostly dark" flag.
async fn file_stats(State(st): State<AppState>, axum::extract::Query(q): axum::extract::Query<PathQuery>) -> Res {
    let root = st.storage.dir();
    match tokio::task::spawn_blocking(move || crate::files::resolve(&root, &q.path).and_then(|p| crate::files::ser_stats(&p, 12))).await {
        Ok(Ok(s)) => Json(json!(s)).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    }
}

#[derive(Deserialize)]
struct DeleteBody { path: String, #[serde(default)] confirm: bool }
/// Deletes one capture. Needs `confirm: true`; refused while recording (the file may be the one being written).
async fn file_delete(State(st): State<AppState>, Json(b): Json<DeleteBody>) -> Res {
    if !b.confirm {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "deleting is permanent: send {\"confirm\": true}" }))).into_response();
    }
    if st.engine.status.borrow().capture.state != "idle" {
        return (StatusCode::CONFLICT, Json(json!({ "error": "a capture is running or still writing" }))).into_response();
    }
    let root = st.storage.dir();
    match tokio::task::spawn_blocking(move || crate::files::delete(&root, &b.path)).await {
        Ok(Ok(freed)) => Json(json!({ "freed": freed })).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    }
}

#[derive(Deserialize)]
struct EjectBody { mount_point: String }
/// Flush, unmount and power off a removable drive. Refused while anything is being recorded or written.
async fn storage_eject(State(st): State<AppState>, Json(b): Json<EjectBody>) -> Res {
    if st.engine.status.borrow().capture.state != "idle" {
        return (StatusCode::CONFLICT, Json(json!({ "error": "a capture is running or still writing: wait until it finishes" }))).into_response();
    }
    let s = st.storage.clone();
    match tokio::task::spawn_blocking(move || s.eject(std::path::Path::new(&b.mount_point))).await {
        Ok(Ok(msg)) => Json(json!({ "message": msg })).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    }
}

#[derive(Deserialize)]
struct Optics {
    focal_length_mm: Option<f64>,
    #[serde(default = "one_f")]
    focal_multiplier: f64,
    pixel_size_um: Option<f64>,
    #[serde(default = "one")]
    binning: u32,
}
fn one() -> u32 { 1 }
fn one_f() -> f64 { 1.0 }
async fn optics_put(s: State<AppState>, Json(b): Json<Optics>) -> Res {
    run(s, Command::SetOptics { focal_length_mm: b.focal_length_mm, focal_multiplier: b.focal_multiplier, pixel_size_um: b.pixel_size_um, binning: b.binning }).await
}

#[derive(Deserialize)]
struct PreviewSettings { stretch: bool }
async fn preview_get(State(st): State<AppState>) -> Json<Value> {
    Json(json!({ "stretch": st.stretch.load(std::sync::atomic::Ordering::Relaxed) }))
}
async fn preview_put(State(st): State<AppState>, Json(b): Json<PreviewSettings>) -> Json<Value> {
    st.stretch.store(b.stretch, std::sync::atomic::Ordering::Relaxed);
    Json(json!({ "stretch": b.stretch }))
}

/// Serial devices that could be a mount: stable `/dev/serial/by-id/*` names when available.
async fn mount_ports() -> Json<Value> {
    let mut ports: Vec<Value> = vec![];
    if let Ok(d) = std::fs::read_dir("/dev/serial/by-id") {
        for e in d.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            ports.push(json!({ "path": format!("/dev/serial/by-id/{name}"), "label": name }));
        }
    }
    if ports.is_empty() {
        for pat in ["ttyACM", "ttyUSB"] {
            if let Ok(d) = std::fs::read_dir("/dev") {
                for e in d.flatten() {
                    let name = e.file_name().to_string_lossy().to_string();
                    if name.starts_with(pat) {
                        ports.push(json!({ "path": format!("/dev/{name}"), "label": name }));
                    }
                }
            }
        }
    }
    ports.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    Json(json!(ports))
}

#[derive(Deserialize)]
struct StartupBody { enabled: bool }
async fn startup_put(s: State<AppState>, Json(b): Json<StartupBody>) -> Res {
    run(s, Command::SetStartup { enabled: b.enabled }).await
}

/// UI preferences (slew speed, form fields, ...) kept on the Pi so they survive reboots and follow the user
/// between devices. Stored under the `ui` key of the settings file.
async fn ui_get(State(st): State<AppState>) -> Json<Value> {
    let file = st.storage.config_file().to_path_buf();
    let v = tokio::task::spawn_blocking(move || crate::config::read(&file).get("ui").cloned().unwrap_or_else(|| json!({}))).await.unwrap_or_default();
    Json(v)
}

#[derive(Deserialize)]
struct UiSetting { key: String, value: Value }
async fn ui_put(State(st): State<AppState>, Json(b): Json<UiSetting>) -> Res {
    let ok_key = !b.key.is_empty() && b.key.len() <= 64 && b.key.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
    if !ok_key || b.value.to_string().len() > 4096 {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid key or value too large" }))).into_response();
    }
    let file = st.storage.config_file().to_path_buf();
    let r = tokio::task::spawn_blocking(move || {
        let mut ui = crate::config::read(&file).get("ui").cloned().and_then(|v| v.as_object().cloned()).unwrap_or_default();
        if ui.len() >= 200 && !ui.contains_key(&b.key) {
            return Err("too many settings".to_string());
        }
        ui.insert(b.key, b.value);
        crate::config::set(&file, "ui", Value::Object(ui))
    })
    .await;
    match r {
        Ok(Ok(())) => Json(json!({})).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    }
}

#[derive(Deserialize, Default)]
struct Confirm { #[serde(default)] confirm: bool }

/// Going home MOVES the mount: refuse unless the caller says it is sure (the UI asks the user first).
async fn mount_motion(s: State<AppState>, body: Option<Json<Confirm>>, cmd: Command) -> Res {
    if !body.map(|b| b.0.confirm).unwrap_or(false) {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "this moves the mount: send {\"confirm\": true}" }))).into_response();
    }
    run(s, cmd).await
}

#[cfg(test)]
mod ble_tests {
    use super::connect_device;

    #[test]
    fn usb_form_is_unchanged() {
        assert_eq!(connect_device(None, Some("/dev/ttyACM0".into())).unwrap().as_deref(), Some("/dev/ttyACM0"));
        assert_eq!(connect_device(Some("usb"), Some(String::new())).unwrap(), None);
    }

    #[test]
    fn ble_address_is_validated_and_prefixed() {
        assert_eq!(connect_device(Some("ble"), Some("aa:bb:cc:dd:ee:01".into())).unwrap().as_deref(), Some("ble:AA:BB:CC:DD:EE:01"));
        assert!(connect_device(Some("ble"), None).is_err());
        assert!(connect_device(Some("ble"), Some("/dev/ttyACM0".into())).is_err());
        assert!(connect_device(Some("ble"), Some("AA:BB:CC:DD:EE".into())).is_err());
        assert!(connect_device(Some("wifi"), None).is_err());
    }
}

#[derive(Deserialize)]
struct GotoSunBody {
    #[serde(default)]
    confirm: bool,
    #[serde(default)]
    allow_unsynced_clock: bool,
}

/// Go to the Sun MOVES the mount (and then switches solar tracking on): explicit confirmation required.
async fn goto_sun(s: State<AppState>, body: Option<Json<GotoSunBody>>) -> Res {
    let b = body.map(|b| b.0).unwrap_or(GotoSunBody { confirm: false, allow_unsynced_clock: false });
    if !b.confirm {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "this moves the mount: send {\"confirm\": true}" }))).into_response();
    }
    run(s, Command::MountGotoSun { allow_unsynced_clock: b.allow_unsynced_clock }).await
}

#[derive(Deserialize)]
struct SiteBody { latitude: f64, longitude: f64 }
async fn site_put(s: State<AppState>, Json(b): Json<SiteBody>) -> Res {
    run(s, Command::SetSite { latitude: b.latitude, longitude: b.longitude }).await
}
