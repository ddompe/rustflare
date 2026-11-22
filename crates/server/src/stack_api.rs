//! The stacker's page: its status, settings and finished stacks. The stacker (`rustflare-stack`) is a separate program that talks to
//! the recorder through files (docs/guide/stacking.md); this module only reads those files for the web page, edits the stacker's
//! settings file, and makes thumbnails. Nothing here can start or stop the stacker: that is `systemctl`'s job.

use crate::api::AppState;
use axum::extract::{Path as UrlPath, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use rustflare_core::{Frame, PixelFormat};
use rustflare_stack::{calib::Settings, session::Entry, ser::Ser, watch::Config};
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};

fn state_dir(st: &AppState) -> PathBuf {
    st.storage.config_file().parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."))
}

fn clean_relative(p: &str) -> bool {
    !p.is_empty() && Path::new(p).components().all(|c| matches!(c, Component::Normal(_)))
}

/// Raw dark and flat recordings in the storage folder (and its date folders), with what they were taken with, so the page can offer them.
fn calibration_files(root: &Path) -> Vec<Value> {
    let mut dirs = vec![root.to_path_buf()];
    if let Ok(rd) = std::fs::read_dir(root) {
        dirs.extend(rd.flatten().map(|e| e.path()).filter(|p| p.is_dir() && p.file_name().map_or(false, |n| n != "stacks" && n != "logs")));
    }
    let mut out = Vec::new();
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let kind = if name.starts_with("dark_") { "dark" } else if name.starts_with("flat_") { "flat" } else { continue };
            if !name.ends_with(".ser") || name.contains("_part") || name.starts_with("._") {
                continue;
            }
            let Ok(ser) = Ser::open(&p) else { continue };
            let s = Settings::from_sidecar(&p);
            out.push(json!({ "path": p.strip_prefix(root).unwrap_or(&p).to_string_lossy(), "kind": kind, "frames": ser.frames(), "exposure_us": s.exposure_us, "gain": s.gain }));
        }
    }
    out.sort_by(|a, b| b["path"].as_str().cmp(&a["path"].as_str()));
    out
}

fn service_state() -> String {
    match std::process::Command::new("systemctl").args(["is-active", "rustflare-stack"]).output() {
        Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(), // active | inactive | failed | activating
        Err(_) => "unknown".into(),
    }
}

fn collect(st: &AppState) -> Value {
    let dir = state_dir(st);
    let root = st.storage.dir();
    let status: Value = std::fs::read_to_string(dir.join("stack-status.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    let manifest: Vec<Entry> = std::fs::read_to_string(root.join("stacks/manifest.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let mut stacks: Vec<(u64, Value)> = manifest
        .iter()
        .filter_map(|e| {
            let mtime = std::fs::metadata(root.join("stacks").join(&e.output)).and_then(|m| m.modified()).ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
            let mut v = serde_json::to_value(e).ok()?;
            v["path"] = json!(format!("stacks/{}", e.output));
            v["thumb"] = json!(e.output);
            v["mtime"] = json!(mtime);
            Some((mtime, v))
        })
        .collect();
    stacks.sort_by(|a, b| b.0.cmp(&a.0));
    let total = stacks.len();
    json!({
        "service": service_state(),
        "status": status,
        "config": Config::load(&dir),
        "stacks": stacks.into_iter().take(48).map(|s| s.1).collect::<Vec<_>>(),
        "total": total,
        "calibration": calibration_files(&root),
    })
}

pub async fn stack_get(State(st): State<AppState>) -> Json<Value> {
    Json(tokio::task::spawn_blocking(move || collect(&st)).await.unwrap_or(Value::Null))
}

/// Changes some of the stacker's settings (any of: enabled, policy, min_gap_s, keep_percent, multipoint, threads, float, flat, darks).
pub async fn stack_config_put(State(st): State<AppState>, Json(patch): Json<Value>) -> Response {
    let path = state_dir(&st).join("stack.json");
    let mut cur: serde_json::Map<String, Value> = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let Some(p) = patch.as_object() else { return (StatusCode::BAD_REQUEST, Json(json!({ "error": "send an object" }))).into_response() };
    for key in ["enabled", "policy", "min_gap_s", "keep_percent", "multipoint", "threads", "float", "flat", "darks"] {
        if let Some(v) = p.get(key) {
            cur.insert(key.into(), v.clone());
        }
    }
    let mut cfg: Config = match serde_json::from_value(Value::Object(cur.clone())) {
        Ok(c) => c,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(json!({ "error": format!("not a valid setting: {e}") }))).into_response(),
    };
    cfg.keep_percent = cfg.keep_percent.clamp(1.0, 100.0);
    cfg.threads = cfg.threads.clamp(1, 4);
    cfg.min_gap_s = cfg.min_gap_s.clamp(0.0, 600.0);
    if cfg.flat.as_deref().map_or(false, |f| !clean_relative(f)) || cfg.darks.iter().any(|d| !clean_relative(d)) {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "flat and dark files are named relative to the storage folder" }))).into_response();
    }
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_vec_pretty(&cfg).unwrap_or_default();
    if let Err(e) = std::fs::write(&tmp, body).and_then(|_| std::fs::rename(&tmp, &path)) {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("could not save: {e}") }))).into_response();
    }
    Json(cfg).into_response()
}

/// A small PNG of a finished stack (display only), made on first request and kept in `stacks/.thumbs/`.
pub async fn stack_thumb(State(st): State<AppState>, UrlPath(rel): UrlPath<String>) -> Response {
    if !clean_relative(&rel) || !rel.ends_with(".fits") {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let root = st.storage.dir().join("stacks");
    let (fits, thumb) = (root.join(&rel), root.join(".thumbs").join(format!("{}.png", rel.replace('/', "__"))));
    let made = tokio::task::spawn_blocking(move || -> Result<PathBuf, String> {
        let newer = |a: &Path, b: &Path| matches!((std::fs::metadata(a).and_then(|m| m.modified()), std::fs::metadata(b).and_then(|m| m.modified())), (Ok(x), Ok(y)) if x >= y);
        if !(thumb.exists() && newer(&thumb, &fits)) {
            let (w, h, px) = rustflare_stack::fits::preview(&fits, 640)?;
            std::fs::create_dir_all(thumb.parent().unwrap()).map_err(|e| e.to_string())?;
            let tmp = thumb.with_extension("png.tmp");
            rustflare_capture::write_png(&tmp, &Frame::new(w as u32, h as u32, PixelFormat::Mono8, px)).map_err(|e| e.to_string())?;
            std::fs::rename(&tmp, &thumb).map_err(|e| e.to_string())?;
        }
        Ok(thumb)
    })
    .await;
    match made {
        Ok(Ok(p)) => match std::fs::read(&p) {
            Ok(bytes) => ([(header::CONTENT_TYPE, "image/png"), (header::CACHE_CONTROL, "max-age=300")], bytes).into_response(),
            Err(_) => StatusCode::NOT_FOUND.into_response(),
        },
        Ok(Err(e)) => (StatusCode::NOT_FOUND, Json(json!({ "error": e }))).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
