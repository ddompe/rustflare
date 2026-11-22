//! Every command sent to the mount and what it answered, one JSON line each, in `<logs>/<date>/mount.jsonl`.
//!
//! Debugging a mount that does something nobody asked for (it slid hundreds of pixels while only the guider was talking to it)
//! needs the exact conversation: which command, when, and the reply. The engine sets the folder once; the drivers write here.

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const DAY_CAP_BYTES: u64 = 40 * 1024 * 1024;

struct Sink {
    dir: Option<PathBuf>,
    writes: u32,
    capped_day: Option<String>,
}

fn sink() -> &'static Mutex<Sink> {
    static S: OnceLock<Mutex<Sink>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Sink { dir: None, writes: 0, capped_day: None }))
}

/// Where to write (the logs folder, e.g. `<storage>/logs`); `None` switches tracing off.
pub fn set_dir(dir: Option<PathBuf>) {
    if let Ok(mut s) = sink().lock() {
        s.dir = dir;
    }
}

fn civil_utc(now: SystemTime) -> (String, String) {
    let d = now.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs() as i64;
    let days = secs.div_euclid(86400) + 719468;
    let era = days.div_euclid(146097);
    let doe = days.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    let rem = secs.rem_euclid(86400);
    (format!("{year:04}-{month:02}-{day:02}"), format!("{:02}:{:02}:{:02}.{:03}", rem / 3600, rem % 3600 / 60, rem % 60, d.subsec_millis()))
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    for c in s.chars().take(160) {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => o.push(' '),
            c => o.push(c),
        }
    }
    o
}

/// Records one exchange: the command text, the reply (or the error) and how long it took.
pub fn record(cmd: &str, reply: Result<&str, &str>, started: Instant) {
    let Ok(mut s) = sink().lock() else { return };
    let Some(dir) = s.dir.clone() else { return };
    let (day, time) = civil_utc(SystemTime::now());
    if s.capped_day.as_deref() == Some(day.as_str()) {
        return;
    }
    let folder = dir.join(&day);
    if std::fs::create_dir_all(&folder).is_err() {
        return;
    }
    let path = folder.join("mount.jsonl");
    s.writes += 1;
    if s.writes % 200 == 1 && std::fs::metadata(&path).map_or(false, |m| m.len() > DAY_CAP_BYTES) {
        s.capped_day = Some(day);
        return;
    }
    let ms = started.elapsed().as_millis();
    let line = match reply {
        Ok(r) => format!("{{\"t\":\"{day}T{time}Z\",\"cmd\":\"{}\",\"reply\":\"{}\",\"ms\":{ms}}}", esc(cmd), esc(r)),
        Err(e) => format!("{{\"t\":\"{day}T{time}Z\",\"cmd\":\"{}\",\"error\":\"{}\",\"ms\":{ms}}}", esc(cmd), esc(e)),
    };
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_one_json_line_per_exchange() {
        let dir = std::env::temp_dir().join(format!("rf-mount-trace-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        set_dir(Some(dir.clone()));
        let t = Instant::now();
        record(":Mge3000#", Ok("1"), t);
        record(":GU#", Err("no reply (timed out)"), t);
        set_dir(None);
        record(":ignored#", Ok("x"), t);
        let file = std::fs::read_dir(&dir).unwrap().next().unwrap().unwrap().path().join("mount.jsonl");
        let text = std::fs::read_to_string(file).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text}");
        assert!(lines[0].contains("\"cmd\":\":Mge3000#\"") && lines[0].contains("\"reply\":\"1\""));
        assert!(lines[1].contains("\"error\":\"no reply (timed out)\""));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
