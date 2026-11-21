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
        .route("/api/mount/drivers", get(|| async { Json(json!(rustflare_mount::DRIVERS)) }))
        .route("/api/mount/connect", post(mount_connect))
        .route("/api/mount/disconnect", post(|s| run(s, Command::DisconnectMount)))
        .route("/api/mount/tracking", post(mount_tracking))
        .route("/api/mount/slew", post(mount_slew))
        .route("/api/mount/stop", post(|s| run(s, Command::MountStop)))
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
        .route("/api/capture/stop", post(|s| run(s, Command::CaptureStop)))
        .route("/api/files", get(files))
        .route("/stream.mjpg", get(mjpeg))
        .route("/snapshot.jpg", get(snapshot))
        .route("/files/*path", get(download))
        .route("/api/storage", get(storage_get).put(storage_put))
        .route("/api/optics", put(optics_put))
        .route("/api/startup", put(startup_put))
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
struct MountConnect { driver: String, device: Option<String> }
async fn mount_connect(s: State<AppState>, Json(b): Json<MountConnect>) -> Res {
    run(s, Command::ConnectMount { driver: b.driver, device: b.device.filter(|d| !d.is_empty()) }).await
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
struct Single { name: Option<String> }
async fn capture_single(s: State<AppState>, Json(b): Json<Single>) -> Res {
    run(s, Command::CaptureSingle { name: b.name }).await
}

#[derive(Deserialize)]
struct Sequence { name: Option<String>, frames: Option<u32>, seconds: Option<f64>, format: Option<String> }
async fn capture_sequence(s: State<AppState>, Json(b): Json<Sequence>) -> Res {
    run(s, Command::CaptureBursts { name: b.name, frames: b.frames, seconds: b.seconds, interval_s: None, bursts: None, format: b.format }).await
}

#[derive(Deserialize)]
struct Bursts { name: Option<String>, frames: Option<u32>, seconds: Option<f64>, interval_s: f64, bursts: Option<u32>, format: Option<String> }
async fn capture_bursts(s: State<AppState>, Json(b): Json<Bursts>) -> Res {
    run(s, Command::CaptureBursts { name: b.name, frames: b.frames, seconds: b.seconds, interval_s: Some(b.interval_s), bursts: b.bursts, format: b.format }).await
}

#[derive(Deserialize)]
struct Buffer { mb: u64 }
async fn capture_buffer(s: State<AppState>, Json(b): Json<Buffer>) -> Res {
    run(s, Command::SetBuffer { mb: b.mb }).await
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
    let stream = futures_util::stream::unfold((rx, std::time::Instant::now() - Duration::from_secs(10)), move |(mut rx, mut last)| async move {
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
                return Some((Ok::<_, std::convert::Infallible>(chunk), (rx, last)));
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
