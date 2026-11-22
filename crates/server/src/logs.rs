//! The always-on logs (events, mount commands) live on the Pi's own disk, not on the capture drive: they must survive
//! the drive being ejected or full, and they must never fill the system disk. This keeps them inside a size and age limit.

use std::path::Path;
use std::time::SystemTime;

/// Largest total size of the day folders kept, and the oldest day kept.
pub const MAX_TOTAL_BYTES: u64 = 200 * 1024 * 1024;
pub const MAX_DAYS: u64 = 14;

fn dir_size(p: &Path) -> u64 {
    std::fs::read_dir(p).map(|rd| rd.flatten().map(|e| e.metadata().map(|m| if m.is_dir() { dir_size(&e.path()) } else { m.len() }).unwrap_or(0)).sum()).unwrap_or(0)
}

/// Deletes the oldest day folders (`YYYY-MM-DD`) of `dir` until the rest is under `max_bytes` and none is older than `max_days`.
/// Today's folder is never deleted. Returns how many folders were removed.
pub fn prune(dir: &Path, max_bytes: u64, max_days: u64, today: &str) -> usize {
    let mut days: Vec<(String, std::path::PathBuf)> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().filter(|e| e.path().is_dir()).filter_map(|e| e.file_name().to_str().filter(|n| n.len() == 10 && n.as_bytes()[4] == b'-').map(|n| (n.to_string(), e.path()))).collect())
        .unwrap_or_default();
    days.sort();
    let mut removed = 0;
    let oldest_kept = SystemTime::now().checked_sub(std::time::Duration::from_secs(max_days * 86400));
    let mut total: u64 = days.iter().map(|(_, p)| dir_size(p)).sum();
    for (name, path) in &days {
        if name == today {
            continue;
        }
        let too_old = oldest_kept.is_some_and(|limit| std::fs::metadata(path).and_then(|m| m.modified()).map_or(false, |t| t < limit));
        if total > max_bytes || too_old {
            let sz = dir_size(path);
            if std::fs::remove_dir_all(path).is_ok() {
                total = total.saturating_sub(sz);
                removed += 1;
            }
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deletes_the_oldest_days_when_over_the_limit_but_never_today() {
        let dir = std::env::temp_dir().join(format!("rf-logs-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for d in ["2026-11-20", "2026-11-21", "2026-11-22"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
            std::fs::write(dir.join(d).join("events.jsonl"), vec![b'x'; 1000]).unwrap();
        }
        std::fs::create_dir_all(dir.join("not-a-day")).unwrap();
        // 3000 bytes in total, limit 2500: only the oldest day goes
        assert_eq!(prune(&dir, 2500, 365, "2026-11-22"), 1);
        assert!(!dir.join("2026-11-20").exists() && dir.join("2026-11-21").exists() && dir.join("2026-11-22").exists() && dir.join("not-a-day").exists());
        // a limit of zero removes everything except today
        assert_eq!(prune(&dir, 0, 365, "2026-11-22"), 1);
        assert!(dir.join("2026-11-22").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
