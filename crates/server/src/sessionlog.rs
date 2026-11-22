//! Opt-in session log for building real-data regression fixtures.
//!
//! While on, about once a second one JSON line is appended to `<storage>/logs/<date>/session_<time>.jsonl` with what
//! the software saw and did: camera fps/level/exposure/gain, the detected Sun, tracking and guiding state, the pulses
//! the guider sent, and the mount's tracking/slewing flags. Every N seconds a small grayscale PNG of the frame is saved
//! next to it, so the detector can be replayed offline (`replay`). Nothing identifying is logged: no site, no device
//! names or addresses, no file paths. A size cap stops the log before it can fill a drive.

use rustflare_core::{Frame, PixelFormat};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Stops logging when the day's folder reaches this size.
const CAP_BYTES: u64 = 200 * 1024 * 1024;
const THUMB_WIDTH: u32 = 320;

#[derive(Default)]
pub struct SessionLog {
    out: Option<std::io::BufWriter<std::fs::File>>,
    stem: Option<PathBuf>,
    frame_every: Option<Duration>,
    last_line: Option<Instant>,
    last_frame: Option<Instant>,
    pulses: Vec<Value>,
    pub lines: u64,
    pub frames: u64,
    pub bytes: u64,
    pub capped: bool,
}

impl SessionLog {
    pub fn enabled(&self) -> bool {
        self.out.is_some()
    }

    pub fn file_name(&self) -> Option<String> {
        self.stem.as_ref().and_then(|s| s.file_name()).map(|n| format!("{}.jsonl", n.to_string_lossy()))
    }

    /// Starts a new log file in `dir` (the logs folder). `frame_every_s`: seconds between thumbnails (None: no frames).
    pub fn start(&mut self, dir: &Path, frame_every_s: Option<f64>) -> Result<(), String> {
        let now = chrono::Utc::now();
        let day = dir.join(now.format("%Y-%m-%d").to_string());
        std::fs::create_dir_all(&day).map_err(|e| format!("{}: {e}", day.display()))?;
        let stem = day.join(format!("session_{}", now.format("%H%M%SZ")));
        let f = std::fs::File::create(stem.with_extension("jsonl")).map_err(|e| e.to_string())?;
        *self = SessionLog { out: Some(std::io::BufWriter::new(f)), stem: Some(stem), frame_every: frame_every_s.filter(|s| *s >= 1.0).map(Duration::from_secs_f64), ..Default::default() };
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(o) = self.out.as_mut() {
            let _ = o.flush();
        }
        self.out = None;
        self.pulses.clear();
    }

    /// Remembers a guide pulse (in multiples of sidereal) for the next line.
    pub fn note_pulse(&mut self, primary: f64, secondary: f64) {
        if self.enabled() && self.pulses.len() < 50 {
            self.pulses.push(json!({ "p": primary, "s": secondary }));
        }
    }

    /// Call for every camera frame; writes at most one line per second. `record` is built lazily by the caller only when due.
    pub fn due(&self, now: Instant) -> bool {
        self.enabled() && self.last_line.map_or(true, |t| now.duration_since(t) >= Duration::from_secs(1))
    }

    pub fn write(&mut self, now: Instant, mut record: Value, frame: &Frame) {
        let Some(out) = self.out.as_mut() else { return };
        self.last_line = Some(now);
        record["pulses"] = Value::Array(std::mem::take(&mut self.pulses));
        record["t_ms"] = json!(chrono::Utc::now().timestamp_millis());
        // a thumbnail every N seconds, referenced from the line
        if let (Some(every), Some(stem)) = (self.frame_every, self.stem.as_ref()) {
            if self.last_frame.map_or(true, |t| now.duration_since(t) >= every) {
                self.last_frame = Some(now);
                let (thumb, scale) = thumbnail(frame);
                let name = format!("{}_f{:05}.png", stem.file_name().unwrap_or_default().to_string_lossy(), self.frames + 1);
                if rustflare_capture::write_png(&stem.with_file_name(&name), &thumb).is_ok() {
                    self.frames += 1;
                    record["frame"] = json!(name);
                    record["scale"] = json!(scale);
                    self.bytes += thumb.data.len() as u64 / 3;
                }
            }
        }
        let mut line = record.to_string();
        line.push('\n');
        if out.write_all(line.as_bytes()).is_ok() {
            self.lines += 1;
            self.bytes += line.len() as u64;
        }
        if self.lines % 30 == 0 {
            let _ = out.flush();
        }
        if self.bytes >= CAP_BYTES {
            self.capped = true;
            self.stop();
        }
    }
}

/// 8-bit grayscale thumbnail by integer decimation to at most `THUMB_WIDTH` wide; returns the decimation factor.
pub fn thumbnail(frame: &Frame) -> (Frame, u32) {
    let f = frame.width.div_ceil(THUMB_WIDTH).max(1);
    let (w, h) = (frame.width / f, frame.height / f);
    let g = frame.to_mono8();
    let mut d = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            d.push(g[((y * f) * frame.width + x * f) as usize]);
        }
    }
    (Frame::new(w, h, PixelFormat::Mono8, d), f)
}

/// Replays the thumbnails of a session log through the Sun detector and compares with what was logged then.
#[derive(Debug, Default)]
#[allow(dead_code)] // used by the replay tests and by developers
pub struct ReplayReport {
    pub checked: u32,
    pub both_found: u32,
    pub lost_now: u32,
    /// Worst centre difference in full-resolution pixels.
    pub max_err_px: f64,
}

#[allow(dead_code)]
pub fn replay(jsonl: &Path) -> std::io::Result<ReplayReport> {
    let text = std::fs::read_to_string(jsonl)?;
    let dir = jsonl.parent().unwrap_or(Path::new("."));
    let mut r = ReplayReport::default();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let (Some(name), Some(scale)) = (v["frame"].as_str(), v["scale"].as_f64()) else { continue };
        let Ok(img) = rustflare_capture::read_png(&dir.join(name)) else { continue };
        r.checked += 1;
        let logged = v["sun"]["x"].as_f64().zip(v["sun"]["y"].as_f64());
        let hint = v["sun"]["r"].as_f64().map(|r| r / scale);
        match (rustflare_tracking::locate_sun(&img, hint), logged) {
            (Some(s), Some((x, y))) => {
                r.both_found += 1;
                r.max_err_px = r.max_err_px.max(((s.pos.x * scale - x).powi(2) + (s.pos.y * scale - y).powi(2)).sqrt());
            }
            (None, Some(_)) => r.lost_now += 1,
            _ => {}
        }
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustflare_core::sim::SimSky;

    /// Real sessions: `RUSTFLARE_FIXTURES=/path/to/logs cargo test -p rustflare-server replay_real -- --nocapture`.
    #[test]
    fn replay_real_logs_if_provided() {
        let Ok(dir) = std::env::var("RUSTFLARE_FIXTURES") else { return };
        for e in walk(Path::new(&dir)) {
            let r = replay(&e).unwrap();
            println!("{}: {r:?}", e.display());
            assert_eq!(r.lost_now, 0, "the detector lost a Sun it found during the session: {}", e.display());
            assert!(r.max_err_px < 12.0, "centre moved by {} px: {}", r.max_err_px, e.display());
        }
    }

    /// Developer tool: `RUSTFLARE_THUMBS=<folder of session PNGs> cargo test -p rustflare-server thumbs_report -- --nocapture`
    /// prints what the Sun detector finds in every picture (centre, radius, partial), to judge its stability.
    #[test]
    fn thumbs_report_if_provided() {
        let Ok(dir) = std::env::var("RUSTFLARE_THUMBS") else { return };
        let mut names: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "png")).collect();
        names.sort();
        let expected: Option<f64> = std::env::var("RUSTFLARE_EXPECTED_R").ok().and_then(|v| v.parse().ok());
        let (mut xs, mut ys, mut rs, mut miss) = (vec![], vec![], vec![], 0);
        for p in &names {
            let f = rustflare_capture::read_png(p).unwrap();
            match rustflare_tracking::locate_sun(&f, expected) {
                Some(s) => { println!("{} x {:6.1} y {:6.1} r {:6.1} partial {}", p.file_name().unwrap().to_string_lossy(), s.pos.x, s.pos.y, s.radius, s.partial); xs.push(s.pos.x); ys.push(s.pos.y); rs.push(s.radius); }
                None => { miss += 1; println!("{} none", p.file_name().unwrap().to_string_lossy()); }
            }
        }
        let sd = |v: &Vec<f64>| { let m = v.iter().sum::<f64>() / v.len().max(1) as f64; (v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / v.len().max(1) as f64).sqrt() };
        println!("found {} of {}; spread (std) x {:.1} y {:.1} r {:.1}", xs.len(), names.len(), sd(&xs), sd(&ys), sd(&rs));
    }

    fn walk(p: &Path) -> Vec<PathBuf> {
        let mut out = vec![];
        if let Ok(rd) = std::fs::read_dir(p) {
            for e in rd.flatten() {
                let q = e.path();
                if q.is_dir() { out.extend(walk(&q)); } else if q.extension().is_some_and(|x| x == "jsonl") { out.push(q); }
            }
        }
        out
    }

    #[test]
    fn log_then_replay_round_trip_on_the_simulated_sun() {
        let dir = std::env::temp_dir().join(format!("rf-slog-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let sky = SimSky::new();
        let mut cam = rustflare_camera::simulator::SimulatorDriver::new(sky);
        let mut c = rustflare_core::CameraDriver::open(&mut cam, "sim0").unwrap();
        let _ = rustflare_core::Camera::set_control(&mut *c, "sim_zoom", 0.9);
        let mut log = SessionLog::default();
        log.start(&dir, Some(1.0)).unwrap();
        let mut t = Instant::now();
        for _ in 0..3 {
            let frame = rustflare_core::Camera::capture(&mut *c, Duration::from_secs(2)).unwrap();
            let sun = rustflare_tracking::locate_sun(&frame, None).expect("the simulated Sun is found");
            t += Duration::from_secs(2);
            assert!(log.due(t) || log.lines == 0);
            log.write(t, json!({ "sun": { "x": sun.pos.x, "y": sun.pos.y, "r": sun.radius } }), &frame);
        }
        log.stop();
        let jsonl = walk(&dir).pop().expect("a log file");
        let r = replay(&jsonl).unwrap();
        assert_eq!(r.checked, 3);
        assert_eq!((r.both_found, r.lost_now), (3, 0), "{r:?}");
        assert!(r.max_err_px < 12.0, "{r:?}");
        let text = std::fs::read_to_string(&jsonl).unwrap();
        assert!(!text.contains("lat") && !text.contains("serial"), "nothing identifying in the log");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
