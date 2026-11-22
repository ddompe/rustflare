//! Telemetry of the stacker's behaviour, for reviewing later how it coexists with a real recording: what CPU, priority, affinity and
//! memory limits it was given, what it actually used, whether it was throttled, and what the machine and the recorder were doing at
//! the time. Written as JSON lines to `<state-dir>/logs/<UTC date>/stack.jsonl`, next to the recorder's own logs (the server's debug
//! API serves them and its pruning keeps them inside the size and age limits). Everything is read from `/proc` and `/sys` and is
//! best effort: on a machine without them (a Mac) the fields are simply missing.

use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn read(p: impl AsRef<Path>) -> Option<String> { std::fs::read_to_string(p).ok() }
fn num(p: impl AsRef<Path>) -> Option<f64> { read(p)?.trim().parse().ok() }
pub fn now_unix() -> f64 { SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0) }

/// The cgroup directory this process runs in (cgroup v2), e.g. `/sys/fs/cgroup/system.slice/rustflare-stack.service`.
fn cgroup_dir() -> Option<PathBuf> {
    let t = read("/proc/self/cgroup")?;
    let line = t.lines().find(|l| l.starts_with("0::"))?;
    Some(PathBuf::from("/sys/fs/cgroup").join(line[3..].trim_start_matches('/')))
}

fn kv_lines(text: &str) -> Map<String, Value> {
    text.lines().filter_map(|l| { let mut it = l.split_whitespace(); let k = it.next()?; Some((k.to_string(), it.next().and_then(|v| v.parse::<f64>().ok()).map_or(Value::Null, |v| json!(v)))) }).collect()
}

/// Fields of `/proc/<pid>/stat` after the command name (field 3 onwards).
fn stat_fields(path: &str) -> Option<Vec<String>> {
    let t = read(path)?;
    Some(t[t.rfind(')')? + 1..].split_whitespace().map(String::from).collect())
}

/// Priority and CPU placement as the kernel sees them.
pub fn environment() -> Value {
    let mut cg = Map::new();
    if let Some(d) = cgroup_dir() {
        cg.insert("path".into(), json!(d.to_string_lossy()));
        for f in ["cpu.max", "cpu.weight", "io.weight", "memory.max", "memory.high", "cpuset.cpus", "cpuset.cpus.effective"] {
            if let Some(v) = read(d.join(f)) {
                cg.insert(f.into(), json!(v.trim()));
            }
        }
    }
    let status = read("/proc/self/status").unwrap_or_default();
    let field = |k: &str| status.lines().find(|l| l.starts_with(k)).map(|l| l[k.len()..].trim().to_string());
    let st = stat_fields("/proc/self/stat");
    json!({
        "cgroup": cg,
        "cgroup_controllers": cgroup_dir().and_then(|d| read(d.join("cgroup.controllers"))).map(|t| t.trim().to_string()),
        "cpus_allowed": field("Cpus_allowed_list:"),
        "nice": st.as_ref().and_then(|f| f.get(16)).and_then(|v| v.parse::<i64>().ok()),
        "cores": std::thread::available_parallelism().map(|n| n.get()).ok(),
        "kernel": read("/proc/sys/kernel/osrelease").map(|s| s.trim().to_string()),
    })
}

/// What systemd was told for this unit (`systemctl show`), or null where there is no systemd.
pub fn unit_properties(unit: &str) -> Value {
    let out = std::process::Command::new("systemctl").args(["show", unit, "-p", "User", "-p", "Nice", "-p", "CPUWeight", "-p", "CPUQuotaPerSecUSec", "-p", "IOSchedulingClass", "-p", "IOWeight", "-p", "MemoryMax", "-p", "AllowedCPUs", "-p", "Restart"]).output();
    match out {
        Ok(o) if o.status.success() => Value::Object(String::from_utf8_lossy(&o.stdout).lines().filter_map(|l| l.split_once('=')).map(|(k, v)| (k.to_string(), json!(v))).collect()),
        _ => Value::Null,
    }
}

/// Counters that only grow; two of them make a delta.
#[derive(Clone, Debug, Default)]
pub struct Counters {
    pub at: Option<Instant>,
    /// Seconds of CPU this process used: user and system.
    pub user_s: f64,
    pub sys_s: f64,
    /// (busy, total) jiffies of every core.
    pub cores: Vec<(f64, f64)>,
    pub cgroup: Map<String, Value>,
    pub psi: Map<String, Value>,
    /// Interrupts per core for the devices that matter to a recording (USB host controllers, SD card, network, PCIe, RP1).
    pub irq: Vec<(String, Vec<f64>)>,
}

/// Interrupt counts per core of the devices that can disturb a recording, from `/proc/interrupts`.
fn irqs() -> Vec<(String, Vec<f64>)> {
    let Some(t) = read("/proc/interrupts") else { return Vec::new() };
    let mut lines = t.lines();
    let ncpu = lines.next().map_or(0, |h| h.split_whitespace().count());
    lines.filter_map(|l| {
        let mut it = l.split_whitespace();
        let id = it.next()?.trim_end_matches(':').to_string();
        let counts: Vec<f64> = it.by_ref().take(ncpu).filter_map(|v| v.parse().ok()).collect();
        let name = it.last()?.to_string();
        let wanted = ["xhci", "mmc", "eth", "pcie", "rp1", "dwc", "nvme", "brcm", "usb"].iter().any(|k| name.contains(k));
        (counts.len() == ncpu && wanted).then(|| (format!("{id}:{name}"), counts))
    }).collect()
}

fn psi() -> Map<String, Value> {
    let mut m = Map::new();
    for r in ["cpu", "io", "memory"] {
        let Some(t) = read(format!("/proc/pressure/{r}")) else { continue };
        for l in t.lines() {
            let mut it = l.split_whitespace();
            let kind = it.next().unwrap_or("some").to_string(); // some | full
            let kv: Map<String, Value> = it.filter_map(|p| p.split_once('=')).filter_map(|(k, v)| Some((k.to_string(), json!(v.parse::<f64>().ok()?)))).collect();
            m.insert(format!("{r}_{kind}"), Value::Object(kv));
        }
    }
    m
}

impl Counters {
    pub fn take() -> Counters {
        let tick = 100.0; // CLK_TCK on Linux
        let st = stat_fields("/proc/self/stat").unwrap_or_default();
        let g = |i: usize| st.get(i).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0) / tick;
        let cores = read("/proc/stat").map(|t| t.lines().filter(|l| l.starts_with("cpu") && l.as_bytes().get(3).map_or(false, |c| c.is_ascii_digit())).map(|l| {
            let v: Vec<f64> = l.split_whitespace().skip(1).filter_map(|x| x.parse().ok()).collect();
            let total: f64 = v.iter().take(8).sum();
            let idle = v.get(3).copied().unwrap_or(0.0) + v.get(4).copied().unwrap_or(0.0);
            (total - idle, total)
        }).collect()).unwrap_or_default();
        let cgroup = cgroup_dir().and_then(|d| read(d.join("cpu.stat"))).map(|t| kv_lines(&t)).unwrap_or_default();
        Counters { at: Some(Instant::now()), user_s: g(11), sys_s: g(12), cores, cgroup, psi: psi(), irq: irqs() }
    }

    /// What happened between `self` (earlier) and `later`.
    pub fn delta(&self, later: &Counters) -> Value {
        let wall = later.at.zip(self.at).map(|(b, a)| b.duration_since(a).as_secs_f64()).unwrap_or(0.0);
        let d = |k: &str| later.cgroup.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0) - self.cgroup.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0);
        let core_busy: Vec<f64> = self.cores.iter().zip(&later.cores).map(|(a, b)| if b.1 > a.1 { ((b.0 - a.0) / (b.1 - a.1) * 100.0 * 10.0).round() / 10.0 } else { 0.0 }).collect();
        let psi_d: Map<String, Value> = later.psi.iter().filter_map(|(k, v)| {
            let (now, then) = (v.get("total")?.as_f64()?, self.psi.get(k).and_then(|o| o.get("total")).and_then(|t| t.as_f64()).unwrap_or(0.0));
            Some((k.clone(), json!(((now - then) / 1000.0).round() / 1000.0))) // microseconds of stall -> seconds
        }).collect();
        // interrupts that fired during the interval, per core, only for the devices that fired
        let irq_d: Map<String, Value> = later.irq.iter().filter_map(|(name, now)| {
            let then = self.irq.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone()).unwrap_or_else(|| vec![0.0; now.len()]);
            let d: Vec<f64> = now.iter().zip(&then).map(|(a, b)| a - b).collect();
            (d.iter().sum::<f64>() > 0.0).then(|| (name.clone(), json!(d)))
        }).collect();
        let cpu = (later.user_s - self.user_s) + (later.sys_s - self.sys_s);
        json!({
            "wall_s": (wall * 1000.0).round() / 1000.0,
            "cpu_user_s": ((later.user_s - self.user_s) * 100.0).round() / 100.0,
            "cpu_sys_s": ((later.sys_s - self.sys_s) * 100.0).round() / 100.0,
            "cpu_cores_used": if wall > 0.0 { (cpu / wall * 100.0).round() / 100.0 } else { 0.0 },
            "cgroup_cpu_usec": d("usage_usec"),
            "cgroup_periods": d("nr_periods"),
            "cgroup_throttled_periods": d("nr_throttled"),
            "cgroup_throttled_s": (d("throttled_usec") / 1e4).round() / 100.0,
            "core_busy_pct": core_busy,
            "system_busy_pct": if core_busy.is_empty() { 0.0 } else { (core_busy.iter().sum::<f64>() / core_busy.len() as f64 * 10.0).round() / 10.0 },
            "psi_stall_s": psi_d,
            "irq_per_core": irq_d,
        })
    }
}

/// The recorder's `capture.state`, with its age.
pub fn recorder(state_dir: &Path) -> Value {
    match read(state_dir.join("capture.state")).and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
        Some(mut v) => {
            let age = v.get("updated").and_then(|u| u.as_f64()).map(|u| now_unix() - u);
            v["age_s"] = json!(age.map(|a| (a * 10.0).round() / 10.0));
            v
        }
        None => Value::Null,
    }
}

/// The firmware's throttling flags (Raspberry Pi): 0x0 = never throttled or under-voltage since boot; bit 0 under-voltage now, bit 2
/// throttled now, bit 3 soft temperature limit now, bits 16 to 19 the same "has occurred since boot".
fn throttled_flags() -> Option<String> {
    if let Ok(o) = std::process::Command::new("vcgencmd").arg("get_throttled").output() {
        if o.status.success() {
            if let Some(v) = String::from_utf8_lossy(&o.stdout).trim().strip_prefix("throttled=") {
                return Some(v.to_string());
            }
        }
    }
    for base in ["/sys/devices/platform", "/sys/devices/platform/soc"] {
        let Ok(rd) = std::fs::read_dir(base) else { continue };
        for e in rd.flatten() {
            for p in [e.path().join("get_throttled"), e.path().join("firmware/get_throttled")] {
                if let Some(t) = read(&p) {
                    return Some(t.trim().to_string());
                }
            }
        }
    }
    None
}

/// An instantaneous reading: the machine, this process, the recorder.
pub fn sample(state_dir: &Path) -> Value {
    let status = read("/proc/self/status").unwrap_or_default();
    let kb = |k: &str| status.lines().find(|l| l.starts_with(k)).and_then(|l| l.split_whitespace().nth(1)).and_then(|v| v.parse::<f64>().ok());
    // which cores this process's threads are on right now
    let mut on = std::collections::BTreeSet::new();
    if let Ok(rd) = std::fs::read_dir("/proc/self/task") {
        for t in rd.flatten() {
            if let Some(f) = stat_fields(&format!("{}/stat", t.path().display())) {
                if f.first().map(|s| s.as_str()) == Some("R") {
                    if let Some(c) = f.get(36).and_then(|v| v.parse::<u32>().ok()) {
                        on.insert(c); // only threads that are running count
                    }
                }
            }
        }
    }
    let cg = cgroup_dir();
    let psi = psi();
    json!({
        "temp_c": num("/sys/class/thermal/thermal_zone0/temp").map(|t| t / 1000.0),
        "freq_mhz": num("/sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq").map(|f| f / 1000.0),
        "throttled": throttled_flags(),
        "load1": read("/proc/loadavg").and_then(|t| t.split_whitespace().next().and_then(|v| v.parse::<f64>().ok())),
        "rss_mb": kb("VmRSS:").map(|k| (k / 1024.0).round()),
        "rss_peak_mb": kb("VmHWM:").map(|k| (k / 1024.0).round()),
        "threads": kb("Threads:"),
        "running_on_cores": on,
        "cgroup_mem_mb": cg.as_ref().and_then(|d| num(d.join("memory.current"))).map(|b| (b / 1048576.0).round()),
        "cgroup_mem_peak_mb": cg.as_ref().and_then(|d| num(d.join("memory.peak"))).map(|b| (b / 1048576.0).round()),
        "psi_avg10": psi.iter().map(|(k, v)| (k.clone(), v.get("avg10").cloned().unwrap_or(Value::Null))).collect::<Map<_, _>>(),
        "recorder": recorder(state_dir),
    })
}

/// Appends JSON lines to `<state-dir>/logs/<UTC date>/stack.jsonl`.
#[derive(Clone)]
pub struct Log {
    dir: PathBuf,
    pub enabled: bool,
}

fn utc_date(t: f64) -> String { crate::output::iso_utc(t)[..10].to_string() }

impl Log {
    pub fn new(state_dir: &Path, enabled: bool) -> Log { Log { dir: state_dir.join("logs"), enabled } }

    pub fn event(&self, kind: &str, fields: Value) {
        if !self.enabled {
            return;
        }
        let t = now_unix();
        let mut obj = Map::new();
        obj.insert("t".into(), json!((t * 1000.0).round() / 1000.0));
        obj.insert("iso".into(), json!(crate::output::iso_utc(t)));
        obj.insert("kind".into(), json!(kind));
        if let Value::Object(f) = fields {
            obj.extend(f);
        }
        let day = self.dir.join(utc_date(t));
        let _ = std::fs::create_dir_all(&day);
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(day.join("stack.jsonl")) {
            use std::io::Write;
            let _ = writeln!(f, "{}", Value::Object(obj));
        }
    }
}

/// Samples every `every` while a job runs, logs each sample and hands them back when stopped.
pub struct Sampler {
    stop: Arc<AtomicBool>,
    handle: std::thread::JoinHandle<Vec<Value>>,
}

impl Sampler {
    pub fn start(state_dir: &Path, log: &Log, every: Duration) -> Sampler {
        let stop = Arc::new(AtomicBool::new(false));
        let (flag, dir, log) = (stop.clone(), state_dir.to_path_buf(), log.clone());
        let handle = std::thread::spawn(move || {
            let mut out = Vec::new();
            let mut last = Instant::now() - every;
            while !flag.load(Ordering::Relaxed) {
                if last.elapsed() >= every {
                    last = Instant::now();
                    let s = sample(&dir);
                    log.event("sample", s.clone());
                    out.push(s);
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            out
        });
        Sampler { stop, handle }
    }

    pub fn stop(self) -> Vec<Value> {
        self.stop.store(true, Ordering::Relaxed);
        self.handle.join().unwrap_or_default()
    }
}

/// A short summary of the samples of one job: extremes, how often the recorder was busy, which cores ran the work.
pub fn summarize(samples: &[Value]) -> Value {
    let nums = |f: &dyn Fn(&Value) -> Option<f64>| -> Vec<f64> { samples.iter().filter_map(f).collect() };
    let max = |v: &[f64]| v.iter().cloned().fold(f64::MIN, f64::max);
    let min = |v: &[f64]| v.iter().cloned().fold(f64::MAX, f64::min);
    let temp = nums(&|s| s["temp_c"].as_f64());
    let freq = nums(&|s| s["freq_mhz"].as_f64());
    let rss = nums(&|s| s["rss_peak_mb"].as_f64());
    let busy: Vec<bool> = samples.iter().filter_map(|s| s["recorder"]["busy"].as_bool()).collect();
    let mut cores = std::collections::BTreeSet::new();
    for s in samples {
        if let Some(a) = s["running_on_cores"].as_array() {
            cores.extend(a.iter().filter_map(|c| c.as_u64()));
        }
    }
    let states: std::collections::BTreeSet<String> = samples.iter().filter_map(|s| s["recorder"]["state"].as_str().map(String::from)).collect();
    json!({
        "samples": samples.len(),
        "temp_c_max": if temp.is_empty() { Value::Null } else { json!(max(&temp)) },
        "freq_mhz_min": if freq.is_empty() { Value::Null } else { json!(min(&freq)) },
        "rss_peak_mb": if rss.is_empty() { Value::Null } else { json!(max(&rss)) },
        "recorder_busy_samples": busy.iter().filter(|b| **b).count(),
        "recorder_states": states,
        "cores_seen_running": cores,
        "throttled_flags_seen": samples.iter().filter_map(|s| s["throttled"].as_str().map(String::from)).collect::<std::collections::BTreeSet<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_are_json_lines_in_a_day_folder_and_the_summary_reads_samples() {
        let dir = std::env::temp_dir().join(format!("rf-telemetry-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let log = Log::new(&dir, true);
        log.event("job", json!({ "source": "a.ser", "wall_s": 1.5 }));
        log.event("state", json!({ "state": "paused" }));
        let day = std::fs::read_dir(dir.join("logs")).unwrap().next().unwrap().unwrap().path();
        let text = std::fs::read_to_string(day.join("stack.jsonl")).unwrap();
        let lines: Vec<Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!((lines.len(), lines[0]["kind"].as_str(), lines[0]["source"].as_str()), (2, Some("job"), Some("a.ser")));
        assert!(lines[0]["t"].as_f64().unwrap() > 1.6e9 && lines[0]["iso"].as_str().unwrap().contains('T'));
        let off = Log::new(&dir.join("off"), false);
        off.event("job", json!({}));
        assert!(!dir.join("off").exists(), "a disabled log writes nothing");
        let s = summarize(&[json!({"temp_c": 60.0, "freq_mhz": 2400.0, "recorder": {"busy": true, "state": "sequence"}, "running_on_cores": [0, 1]}), json!({"temp_c": 63.5, "freq_mhz": 1500.0, "recorder": {"busy": false, "state": "idle"}, "running_on_cores": [1, 3]})]);
        assert_eq!((s["temp_c_max"].as_f64(), s["freq_mhz_min"].as_f64(), s["recorder_busy_samples"].as_u64()), (Some(63.5), Some(1500.0), Some(1)));
        assert_eq!(s["cores_seen_running"], json!([0, 1, 3]));
        let (a, b) = (Counters::take(), Counters::take());
        assert!(a.delta(&b)["wall_s"].as_f64().unwrap() >= 0.0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
