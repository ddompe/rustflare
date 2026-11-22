//! A background writer for log lines and small state files, so the engine thread (which handles every camera frame) never waits on
//! the Pi's SD card. Lines go through a bounded channel to a thread at the lowest CPU priority; if the writer falls behind the line
//! is dropped, never the capture.

use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

enum Job {
    /// Append `text` plus a newline to `path` (the folder is created; nothing is written once the file is over `cap` bytes, if given).
    Append { path: PathBuf, text: String, cap: Option<u64> },
    /// Replace the whole file atomically (write beside it, rename).
    Replace { path: PathBuf, text: String },
}

#[derive(Clone)]
pub struct AsyncLog {
    tx: SyncSender<Job>,
    dropped: Arc<AtomicU64>,
}

/// Lowest CPU priority for the calling thread (Linux: nice is per thread).
fn be_nice() {
    #[cfg(target_os = "linux")]
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, 0, 19);
    }
}

impl AsyncLog {
    pub fn new() -> AsyncLog {
        let (tx, rx) = sync_channel::<Job>(2048);
        std::thread::Builder::new()
            .name("logwriter".into())
            .spawn(move || {
                be_nice();
                while let Ok(job) = rx.recv() {
                    match job {
                        Job::Append { path, text, cap } => {
                            if cap.map_or(false, |c| std::fs::metadata(&path).map_or(false, |m| m.len() > c)) {
                                continue;
                            }
                            if let Some(dir) = path.parent() {
                                if std::fs::create_dir_all(dir).is_err() {
                                    continue;
                                }
                            }
                            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
                                let _ = writeln!(f, "{text}");
                            }
                        }
                        Job::Replace { path, text } => {
                            let tmp = path.with_extension("tmp");
                            let _ = std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, &path));
                        }
                    }
                }
            })
            .expect("spawn logwriter");
        AsyncLog { tx, dropped: Arc::new(AtomicU64::new(0)) }
    }

    fn send(&self, job: Job) {
        if let Err(TrySendError::Full(_)) = self.tx.try_send(job) {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn append(&self, path: PathBuf, text: String, cap: Option<u64>) { self.send(Job::Append { path, text, cap }) }
    pub fn replace(&self, path: PathBuf, text: String) { self.send(Job::Replace { path, text }) }
    pub fn dropped(&self) -> u64 { self.dropped.load(Ordering::Relaxed) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_written_in_order_off_the_calling_thread_and_a_cap_stops_a_runaway_file() {
        let dir = std::env::temp_dir().join(format!("rf-asynclog-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let log = AsyncLog::new();
        let f = dir.join("day/events.jsonl");
        for i in 0..5 {
            log.append(f.clone(), format!("line {i}"), None);
        }
        log.replace(dir.join("state"), "idle".into());
        for _ in 0..100 {
            if std::fs::read_to_string(dir.join("state")).is_ok() && std::fs::read_to_string(&f).map_or(false, |t| t.lines().count() == 5) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "line 0\nline 1\nline 2\nline 3\nline 4\n");
        assert_eq!(std::fs::read_to_string(dir.join("state")).unwrap(), "idle");
        let capped = dir.join("capped.jsonl");
        log.append(capped.clone(), "x".repeat(100), Some(50));
        log.append(capped.clone(), "second".into(), Some(50)); // the file is over the cap now: skipped
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert_eq!(std::fs::read_to_string(&capped).unwrap().lines().count(), 1);
        assert_eq!(log.dropped(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
