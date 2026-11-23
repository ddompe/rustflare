//! `rustflare-stack watch`: stack recordings as they finish, at low priority, without disturbing a recording.
//!
//! The recorder (rustflare-server) writes `capture.state` into its settings folder (is it busy, where does it write) and a
//! `<recording>.ready` marker beside each finished recording. The stacker reads those, never talks to the recorder, and keeps its
//! own settings in `stack.json` and its status in `stack-status.json` in the same folder.

use crate::calib::CalSet;
use crate::ser::Ser;
use crate::session::{self, Entry};
use crate::stack::Options;
use crate::telemetry::{self, Counters, Log, Sampler};
use serde_json::json;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Policy {
    /// Work only while nothing is recording (default for a Pi that is also the recorder: safest).
    WhenIdle,
    /// Work also in the gaps of a recording that waits for its next burst, when the gap is long enough.
    BetweenBursts,
    /// Work whenever there is something to do (the stacker is still nice 19).
    Always,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    pub enabled: bool,
    pub policy: Policy,
    /// The shortest gap (seconds) in which `between_bursts` starts a stack.
    pub min_gap_s: f64,
    pub keep_percent: f64,
    pub multipoint: bool,
    pub threads: usize,
    /// 32-bit float FITS (otherwise unsigned 16-bit).
    pub float: bool,
    /// Raw flat and dark recordings (SER), absolute or relative to the recorder's storage folder.
    pub flat: Option<String>,
    pub darks: Vec<String>,
    /// Also stack finished recordings that have no `.ready` marker (made before the marker existed, or by other programs). Off by
    /// default, so that switching the service on does not start on every old recording on the drive.
    pub include_unmarked: bool,
    /// Keep telemetry (CPU, priority, throttling, temperature, what the recorder was doing) in `logs/<date>/stack.jsonl`.
    pub telemetry: bool,
    /// Prominence lanes: draw a black disc over the Sun after stacking (the recorded frames are raw), and its size as a multiple of the radius.
    pub disc_mask: bool,
    pub disc_scale: f64,
}

impl Default for Config {
    fn default() -> Self {
        Config { enabled: true, policy: Policy::BetweenBursts, min_gap_s: 15.0, keep_percent: 25.0, multipoint: true, threads: 2, float: true, flat: None, darks: Vec::new(), include_unmarked: false, telemetry: true, disc_mask: true, disc_scale: 1.03 }
    }
}

impl Config {
    pub fn load(dir: &Path) -> Config {
        std::fs::read_to_string(dir.join("stack.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn options(&self) -> Options {
        Options { keep: (self.keep_percent / 100.0).clamp(0.01, 1.0), multipoint: self.multipoint, threads: self.threads.max(1), disc_mask: self.disc_mask, disc_scale: self.disc_scale.clamp(0.8, 1.2), ..Options::default() }
    }
}

/// What the recorder says about itself (`capture.state`).
#[derive(Deserialize, Clone, Debug)]
pub struct Recorder {
    pub state: String,
    pub busy: bool,
    pub next_in_s: Option<f64>,
    pub dir: String,
    pub updated: f64,
}

pub fn read_recorder(dir: &Path) -> Option<Recorder> {
    serde_json::from_str(&std::fs::read_to_string(dir.join("capture.state")).ok()?).ok()
}

/// A recorder whose file has not been refreshed for this long is taken to be stopped.
const STALE_S: f64 = 40.0;

/// Whether the stacker may work now, and why not. A stale or missing state file means the recorder is not running.
pub fn may_work(cfg: &Config, rec: Option<&Recorder>, now_unix: f64) -> (bool, &'static str) {
    let Some(r) = rec.filter(|r| now_unix - r.updated <= STALE_S) else { return (true, "the recorder is not running") };
    match cfg.policy {
        Policy::Always => (true, "always"),
        _ if r.busy => (false, "a recording is writing frames"),
        Policy::WhenIdle => if r.state == "idle" { (true, "idle") } else { (false, "a recording is under way") },
        Policy::BetweenBursts => match r.next_in_s {
            Some(g) if g < cfg.min_gap_s => (false, "the next burst is too close"),
            _ => (true, "between bursts"),
        },
    }
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Status {
    /// "idle" | "working" | "paused" | "disabled"
    pub state: String,
    pub reason: String,
    pub current: Option<String>,
    pub pending: usize,
    pub done: usize,
    pub last_error: Option<String>,
    pub last: Option<Entry>,
    pub root: String,
    pub updated: f64,
}

fn now_unix() -> f64 { SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0) }

fn write_status(dir: &Path, st: &Status) {
    let path = dir.join("stack-status.json");
    let tmp = path.with_extension("json.tmp");
    let _ = std::fs::write(&tmp, serde_json::to_vec_pretty(st).unwrap_or_default()).and_then(|_| std::fs::rename(&tmp, &path));
}

/// Recordings that are finished and not stacked yet: those with a `.ready` marker, and, if `include_unmarked`, complete SER files
/// (from older recorders or other programs) that have not been touched for ten minutes.
pub fn pending(root: &Path, out: &Path, failed: &HashSet<PathBuf>, include_unmarked: bool) -> Vec<PathBuf> {
    // bursts the recorder stacked from memory are in the manifest already: their video, if it was kept, is not stacked a second time
    let done: HashSet<String> = std::fs::read_to_string(out.join("manifest.json")).ok().and_then(|t| serde_json::from_str::<Vec<Entry>>(&t).ok()).unwrap_or_default().into_iter().map(|e| e.source).collect();
    session::find_recordings(root)
        .into_iter()
        .filter(|p| !failed.contains(p) && !session::is_stacked(root, out, p) && !done.contains(&*p.strip_prefix(root).unwrap_or(p).to_string_lossy()))
        .filter(|p| {
            if rustflare_ready_marker(p).exists() {
                return true;
            }
            if !include_unmarked {
                return false;
            }
            let old = std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).map_or(false, |a| a > Duration::from_secs(600));
            old && Ser::open(p).map(|s| s.is_complete()).unwrap_or(false)
        })
        .collect()
}

/// `<name>.ready` (kept in step with `rustflare_capture::sink::ready_marker_path`).
fn rustflare_ready_marker(recording: &Path) -> PathBuf {
    let mut n = recording.as_os_str().to_os_string();
    n.push(".ready");
    PathBuf::from(n)
}

fn resolve(root: &Path, p: &str) -> PathBuf {
    let pb = PathBuf::from(p);
    if pb.is_absolute() { pb } else { root.join(pb) }
}

/// Run until stopped (or, with `once`, until nothing is left to do and the policy allows it). `fallback_root` is used when the
/// recorder's state file does not say where it writes.
pub fn watch(state_dir: &Path, fallback_root: Option<&Path>, once: bool, log: &dyn Fn(&str)) {
    let mut failed: HashSet<PathBuf> = HashSet::new();
    let mut status = Status::default();
    let mut cals: Option<(String, CalSet)> = None; // keyed by the calibration settings, rebuilt when they change
    let mut tlog = Log::new(state_dir, Config::load(state_dir).telemetry);
    tlog.event("start", json!({ "version": env!("CARGO_PKG_VERSION"), "environment": telemetry::environment(), "unit": telemetry::unit_properties("rustflare-stack"), "config": Config::load(state_dir), "sample": telemetry::sample(state_dir) }));
    let (mut last_key, mut last_idle_beat) = (String::new(), Instant::now());
    if !once {
        let dir = state_dir.to_path_buf();
        std::thread::spawn(move || crate::serve::run(&dir, &|m| println!("{m}")));
    }
    loop {
        let cfg = Config::load(state_dir);
        tlog.enabled = cfg.telemetry;
        let rec = read_recorder(state_dir);
        let root = rec.as_ref().filter(|r| now_unix() - r.updated <= STALE_S).map(|r| PathBuf::from(&r.dir)).or_else(|| fallback_root.map(|p| p.to_path_buf()));
        status.root = root.as_ref().map(|r| r.to_string_lossy().to_string()).unwrap_or_default();
        let mut sleep = Duration::from_secs(5);
        match root.as_ref().filter(|r| r.is_dir()) {
            None => {
                status.state = "idle".into();
                status.reason = "no storage folder".into();
                status.pending = 0;
            }
            Some(_) if !cfg.enabled => {
                status.state = "disabled".into();
                status.reason = "switched off in stack.json".into();
            }
            Some(root) => {
                let out = root.join("stacks");
                let todo = pending(root, &out, &failed, cfg.include_unmarked);
                status.pending = todo.len();
                let (ok, why) = may_work(&cfg, rec.as_ref(), now_unix());
                if todo.is_empty() {
                    status.state = "idle".into();
                    status.reason = "nothing to stack".into();
                    status.current = None;
                    if once {
                        write_status(state_dir, &status);
                        tlog.event("state", json!({ "state": status.state, "reason": status.reason, "pending": status.pending, "policy": cfg.policy, "recorder": telemetry::recorder(state_dir) }));
                        return;
                    }
                } else if !ok || (crate::serve::CONNECTED.load(std::sync::atomic::Ordering::Relaxed) && rec.as_ref().map_or(false, |r| r.state != "idle")) {
                    status.state = "paused".into();
                    status.reason = if ok { "stacking from memory while a recording is under way" } else { why }.into();
                    status.current = None;
                    if once {
                        write_status(state_dir, &status);
                        tlog.event("state", json!({ "state": status.state, "reason": status.reason, "pending": status.pending, "policy": cfg.policy, "recorder": telemetry::recorder(state_dir) }));
                        return;
                    }
                    sleep = Duration::from_secs(3);
                } else {
                    let key = format!("{:?}{:?}", cfg.flat, cfg.darks);
                    if cals.as_ref().map(|c| c.0 != key).unwrap_or(true) {
                        let darks: Vec<PathBuf> = cfg.darks.iter().map(|d| resolve(root, d)).collect();
                        match CalSet::load(&darks, cfg.flat.as_ref().map(|f| resolve(root, f)).as_deref()) {
                            Ok(c) => cals = Some((key, c)),
                            Err(e) => {
                                status.last_error = Some(format!("calibration: {e}"));
                                log(&format!("calibration: {e}"));
                                cals = Some((key, CalSet::default())); // stack without calibration rather than not at all
                            }
                        }
                    }
                    let src = todo[0].clone();
                    let rel = src.strip_prefix(root).unwrap_or(&src).to_string_lossy().to_string();
                    status.state = "working".into();
                    status.reason = why.into();
                    status.current = Some(rel.clone());
                    write_status(state_dir, &status);
                    let t = Instant::now();
                    let (before, sampler) = (Counters::take(), Sampler::start(state_dir, &tlog, Duration::from_secs(2)));
                    let rec_before = telemetry::recorder(state_dir);
                    let _one_at_a_time = crate::serve::WORK.lock().unwrap();
                    let result = session::stack_one(root, &out, &src, &cfg.options(), &cals.as_ref().unwrap().1, cfg.float).and_then(|e| session::record(&out, &e).map(|_| e));
                    drop(_one_at_a_time);
                    let (samples, after) = (sampler.stop(), Counters::take());
                    let mut job = before.delta(&after);
                    job["source"] = json!(rel);
                    job["ok"] = json!(result.is_ok());
                    job["error"] = json!(result.as_ref().err());
                    job["policy"] = json!(cfg.policy);
                    job["threads_asked"] = json!(cfg.threads);
                    job["multipoint"] = json!(cfg.multipoint);
                    job["why_allowed"] = json!(why);
                    job["recorder_before"] = rec_before;
                    job["recorder_after"] = telemetry::recorder(state_dir);
                    job["during"] = telemetry::summarize(&samples);
                    job["environment"] = telemetry::environment();
                    if let Ok(e) = &result {
                        job["frames"] = json!(e.frames);
                        job["kept"] = json!(e.kept);
                        job["phases"] = json!(e.phases);
                        job["signal"] = json!(e.signal);
                    }
                    tlog.event("job", job);
                    match result {
                        Ok(e) => {
                            log(&format!("{rel}: {} of {} frames in {:.1} s", e.kept, e.frames, t.elapsed().as_secs_f64()));
                            status.done += 1;
                            status.last = Some(e);
                            status.last_error = None;
                        }
                        Err(e) => {
                            log(&format!("{rel}: {e}"));
                            status.last_error = Some(format!("{rel}: {e}"));
                            failed.insert(src);
                        }
                    }
                    sleep = Duration::from_millis(200); // straight on to the next one
                }
            }
        }
        status.updated = now_unix();
        write_status(state_dir, &status);
        // every change of what the stacker is doing (and why) is a telemetry event, with what the recorder was doing at that moment
        let key = format!("{}|{}", status.state, status.reason);
        if key != last_key {
            last_key = key;
            last_idle_beat = Instant::now();
            tlog.event("state", json!({ "state": status.state, "reason": status.reason, "pending": status.pending, "policy": cfg.policy, "recorder": telemetry::recorder(state_dir) }));
        } else if status.state != "working" && last_idle_beat.elapsed() >= Duration::from_secs(60) {
            last_idle_beat = Instant::now();
            tlog.event("idle", telemetry::sample(state_dir)); // a slow heartbeat: temperature, load and the recorder's state between jobs
        }
        std::thread::sleep(sleep);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(state: &str, busy: bool, next: Option<f64>, age: f64) -> Recorder {
        Recorder { state: state.into(), busy, next_in_s: next, dir: "/x".into(), updated: 1000.0 - age }
    }

    #[test]
    fn the_policy_never_works_over_a_burst() {
        let mut c = Config::default();
        let now = 1000.0;
        // the recorder is not running (no file, or a stale one): free to work
        assert!(may_work(&c, None, now).0);
        assert!(may_work(&c, Some(&rec("sequence", true, None, 120.0)), now).0);
        // frames are being written: never, under any policy but the explicit "always"
        for p in [Policy::WhenIdle, Policy::BetweenBursts] {
            c.policy = p;
            assert!(!may_work(&c, Some(&rec("sequence", true, None, 1.0)), now).0);
            assert!(!may_work(&c, Some(&rec("flushing", true, None, 1.0)), now).0);
        }
        // a recording waiting for its next burst
        c.policy = Policy::WhenIdle;
        assert!(!may_work(&c, Some(&rec("sequence", false, Some(60.0), 1.0)), now).0);
        c.policy = Policy::BetweenBursts;
        c.min_gap_s = 15.0;
        assert!(may_work(&c, Some(&rec("sequence", false, Some(60.0), 1.0)), now).0);
        assert!(!may_work(&c, Some(&rec("sequence", false, Some(5.0), 1.0)), now).0, "a gap shorter than a stack");
        // idle
        assert!(may_work(&c, Some(&rec("idle", false, None, 1.0)), now).0);
        c.policy = Policy::Always;
        assert!(may_work(&c, Some(&rec("sequence", true, None, 1.0)), now).0);
    }

    #[test]
    fn the_config_has_safe_defaults_and_reads_partial_files() {
        let c: Config = serde_json::from_str(r#"{"keep_percent": 10}"#).unwrap();
        assert_eq!((c.keep_percent, c.policy, c.threads, c.enabled), (10.0, Policy::BetweenBursts, 2, true));
        let c: Config = serde_json::from_str(r#"{"policy":"when_idle","flat":"cal/flat.ser"}"#).unwrap();
        assert_eq!((c.policy, c.flat.as_deref()), (Policy::WhenIdle, Some("cal/flat.ser")));
    }
}
