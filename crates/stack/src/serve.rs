//! Stacking bursts straight from the recorder's frame ring (ADR 0019). The recorder listens on `stack.sock` in its settings folder; this
//! side connects (and reconnects, whichever restarts), receives the ring's file descriptor, and is told which frames to stack. The
//! result (a FITS still and a score for every frame) goes back, so that the recorder knows what to keep of the raw video.
//!
//! Messages are JSON in the length-prefixed frames of `rustflare_ring::ipc`:
//!   recorder -> stacker  `{"t":"ring","ring":N}` (with the descriptor), `{"t":"burst","ring":N,"id":B,"first":S,"count":C,"stem":"rel/path.ser","keep_percent":25,"exposure_us":..,"gain":..}`
//!   stacker -> recorder  `{"t":"hello",...}`, `{"t":"done","id":B,"ok":true,"output":..,"seconds":..,"frames":..,"kept":..,"scores":[..]}` or `{"t":"done","id":B,"ok":false,"error":..}`

use crate::calib::{CalSet, Settings};
use crate::session;
use crate::source::RingFrames;
use crate::telemetry::{Counters, Log};
use crate::watch::{read_recorder, Config};
use rustflare_ring::ipc::{self, Receiver};
use rustflare_ring::Ring;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Only one stack runs at a time, whether it comes from the ring or from the disk.
pub static WORK: Mutex<()> = Mutex::new(());
/// The recorder is connected: the disk watcher keeps out of the way while a recording is under way.
pub static CONNECTED: AtomicBool = AtomicBool::new(false);

pub fn socket_path(state_dir: &Path) -> PathBuf { state_dir.join("stack.sock") }

/// Runs until the process ends: connect, serve, reconnect.
pub fn run(state_dir: &Path, log: &(dyn Fn(&str) + Sync)) {
    let tlog = Log::new(state_dir, true);
    let mut cals: Option<(String, CalSet)> = None;
    loop {
        match UnixStream::connect(socket_path(state_dir)) {
            Ok(sock) => {
                CONNECTED.store(true, Ordering::Relaxed);
                log("connected to the recorder");
                tlog.event("serve_connected", json!({}));
                let why = connection(state_dir, sock, &tlog, &mut cals, log);
                CONNECTED.store(false, Ordering::Relaxed);
                log(&format!("recorder connection ended: {why}"));
                tlog.event("serve_disconnected", json!({ "why": why }));
            }
            Err(_) => {}
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

fn connection(state_dir: &Path, sock: UnixStream, tlog: &Log, cals: &mut Option<(String, CalSet)>, log: &(dyn Fn(&str) + Sync)) -> String {
    let Ok(tx) = sock.try_clone() else { return "cannot clone the socket".into() };
    let mut rx = Receiver::new(sock);
    let say = |v: Value| ipc::send_text(&tx, &v.to_string(), None);
    if say(json!({ "t": "hello", "pid": std::process::id(), "version": env!("CARGO_PKG_VERSION") })).is_err() {
        return "the recorder went away".into();
    }
    let mut rings: HashMap<u64, Ring> = HashMap::new();
    loop {
        let msg = match rx.recv() {
            Ok(Some(m)) => m,
            Ok(None) => return "closed".into(),
            Err(e) => return format!("read error: {e}"),
        };
        let Ok(v) = serde_json::from_slice::<Value>(&msg) else { continue };
        match v["t"].as_str().unwrap_or("") {
            "ring" => {
                let no = v["ring"].as_u64().unwrap_or(0);
                if let Some(fd) = rx.take_fd() {
                    match Ring::from_fd(std::os::fd::AsFd::as_fd(&fd)) {
                        Ok(r) => {
                            rings.insert(no, r);
                            while rings.len() > 3 {
                                let old = *rings.keys().min().unwrap();
                                rings.remove(&old);
                            }
                        }
                        Err(e) => log(&format!("ring {no}: {e}")),
                    }
                }
            }
            "burst" => {
                let id = v["id"].as_u64().unwrap_or(0);
                let result = burst(state_dir, &v, &rings, tlog, cals);
                let reply = match result {
                    Ok(mut r) => {
                        r["t"] = json!("done");
                        r["id"] = json!(id);
                        r["ok"] = json!(true);
                        r
                    }
                    Err(e) => {
                        log(&format!("burst {id}: {e}"));
                        json!({ "t": "done", "id": id, "ok": false, "error": e })
                    }
                };
                if say(reply).is_err() {
                    return "the recorder went away while a stack was being sent".into();
                }
            }
            _ => {}
        }
    }
}

fn burst(state_dir: &Path, v: &Value, rings: &HashMap<u64, Ring>, tlog: &Log, cals: &mut Option<(String, CalSet)>) -> Result<Value, String> {
    let cfg = Config::load(state_dir);
    if !cfg.enabled {
        return Err("the stacker is switched off (stack.json)".into());
    }
    let ring = rings.get(&v["ring"].as_u64().unwrap_or(0)).ok_or("the burst is in a ring this stacker was not given")?;
    let (first, count) = (v["first"].as_u64().ok_or("no first frame")?, v["count"].as_u64().ok_or("no frame count")? as usize);
    let stem = v["stem"].as_str().ok_or("no name")?;
    let root = read_recorder(state_dir).map(|r| PathBuf::from(r.dir)).ok_or("the recorder does not say where it writes")?;
    let consumer = ring.attach_consumer("stacker", false, false).map_err(|e| e.to_string())?;
    let src = RingFrames::new(consumer, first, count)?;
    let mut opts = cfg.options();
    if let Some(k) = v["keep_percent"].as_f64() {
        opts.keep = (k / 100.0).clamp(0.01, 1.0);
    }
    let key = format!("{:?}{:?}", cfg.flat, cfg.darks);
    if cals.as_ref().map_or(true, |c| c.0 != key) {
        let darks: Vec<PathBuf> = cfg.darks.iter().map(|d| resolve(&root, d)).collect();
        *cals = Some((key, CalSet::load(&darks, cfg.flat.as_ref().map(|f| resolve(&root, f)).as_deref()).unwrap_or_default()));
    }
    let settings = Settings { exposure_us: v["exposure_us"].as_f64(), gain: v["gain"].as_f64() };
    let _one_at_a_time = WORK.lock().unwrap();
    let (before, t) = (Counters::take(), Instant::now());
    let out = root.join("stacks");
    let result = session::stack_source(&root, &out, &root.join(stem), &src, settings, &opts, &cals.as_ref().unwrap().1, cfg.float);
    // frames the camera overwrote while we worked make the stack unusable: say so rather than hand back a torn one
    let result = result.and_then(|r| if src.intact() { Ok(r) } else { Err("part of the burst was overwritten in the ring while it was being stacked".to_string()) });
    let mut job = before.delta(&Counters::take());
    job["source"] = json!(format!("ring:{stem}"));
    job["ok"] = json!(result.is_ok());
    job["error"] = json!(result.as_ref().err());
    job["frames"] = json!(count);
    job["seconds"] = json!(t.elapsed().as_secs_f64());
    tlog.event("job", job);
    let (entry, scores) = result?;
    session::record(&out, &entry)?;
    Ok(json!({ "output": entry.output, "seconds": entry.seconds, "frames": entry.frames, "kept": entry.kept, "scores": scores, "signal": entry.signal }))
}

fn resolve(root: &Path, p: &str) -> PathBuf {
    let pb = PathBuf::from(p);
    if pb.is_absolute() { pb } else { root.join(pb) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustflare_ring::{Geometry, Ring};
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixListener;

    fn frame(w: usize, h: usize, k: usize) -> Vec<u8> {
        // a bright disc with texture and a little noise, shifted a pixel per frame
        let mut out = Vec::with_capacity(w * h * 2);
        let mut seed = (k as u32 + 1) * 2654435761;
        for y in 0..h {
            for x in 0..w {
                let (dx, dy) = (x as f64 - 64.0 - (k % 3) as f64, y as f64 - 64.0);
                let r = (dx * dx + dy * dy).sqrt();
                let tex = ((x * 7 + y * 3) % 11) as f64 * 40.0;
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let v = if r < 40.0 { 30000.0 + tex } else { 800.0 } + (seed >> 28) as f64 * 20.0;
                out.extend_from_slice(&(v as u16).to_le_bytes());
            }
        }
        out
    }

    #[test]
    fn a_burst_in_the_ring_is_stacked_and_scored_and_the_answer_comes_back() {
        let dir = std::env::temp_dir().join(format!("rustflare-serve-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("data")).unwrap();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        std::fs::write(dir.join("capture.state"), format!(r#"{{"state":"idle","busy":false,"next_in_s":null,"dir":"{}","updated":{now}}}"#, dir.join("data").display())).unwrap();
        std::fs::write(dir.join("stack.json"), r#"{"threads":1,"keep_percent":50}"#).unwrap();
        let listener = UnixListener::bind(socket_path(&dir)).unwrap();
        let d2 = dir.clone();
        std::thread::spawn(move || run(&d2, &|_| {}));
        let (conn, _) = listener.accept().unwrap();
        let mut rx = Receiver::new(conn.try_clone().unwrap());
        let hello: Value = serde_json::from_slice(&rx.recv().unwrap().unwrap()).unwrap();
        assert_eq!(hello["t"], "hello");
        // the recorder's side: a ring with a burst of 12 frames
        let (w, h) = (128usize, 128usize);
        let (ring, fd) = Ring::create(Geometry { width: w as u32, height: h as u32, bits: 16, format: 0 }, 20).unwrap();
        let mut p = ring.become_producer();
        for k in 0..12 {
            p.push(&frame(w, h, k), Default::default()).unwrap();
        }
        ipc::send_text(&conn, r#"{"t":"ring","ring":1}"#, Some(fd.as_raw_fd())).unwrap();
        ipc::send_text(&conn, r#"{"t":"burst","ring":1,"id":7,"first":1,"count":12,"stem":"2026-01-01/b.ser","keep_percent":50,"exposure_us":1000,"gain":100}"#, None).unwrap();
        let done: Value = serde_json::from_slice(&rx.recv().unwrap().unwrap()).unwrap();
        assert_eq!(done["t"], "done");
        assert_eq!(done["id"], 7);
        assert_eq!(done["ok"], true, "{done}");
        assert_eq!(done["scores"].as_array().unwrap().len(), 12, "a score for every frame");
        assert_eq!(done["kept"], 6);
        let out = dir.join("data/stacks").join(done["output"].as_str().unwrap());
        assert!(out.exists() && std::fs::metadata(&out).unwrap().len() > (w * h * 4) as u64, "{}", out.display());
        // a burst that is not in the ring any more is an error the recorder can act on, not a bad stack
        for k in 12..40 {
            p.push(&frame(w, h, k), Default::default()).unwrap();
        }
        ipc::send_text(&conn, r#"{"t":"burst","ring":1,"id":8,"first":1,"count":12,"stem":"2026-01-01/c.ser","keep_percent":50}"#, None).unwrap();
        let done: Value = serde_json::from_slice(&rx.recv().unwrap().unwrap()).unwrap();
        assert_eq!((done["id"].as_u64(), done["ok"].as_bool()), (Some(8), Some(false)), "{done}");
        assert!(done["error"].as_str().unwrap().contains("overwritten"), "{done}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
