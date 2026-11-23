//! The recorder's end of the link to the stacker (ADR 0019). The recorder listens on `stack.sock` in its settings folder; the stacker
//! (`rustflare-stack watch`) connects, and keeps reconnecting if either side restarts. Over it go the frame ring's file descriptor and
//! the burst jobs; back come the finished stacks. A broken link is reported as an event, never silently dropped: the engine falls back
//! to writing the raw video of every burst that was waiting for a stack.

use rustflare_ring::ipc::{self, Receiver};
use serde_json::Value;
use std::os::fd::RawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub enum Event {
    Connected,
    Disconnected,
    /// A `done` message from the stacker.
    Done(Value),
}

struct Shared {
    conn: Mutex<Option<(u64, UnixStream)>>,
    events: Mutex<Vec<Event>>,
    generation: AtomicU64,
}

#[derive(Clone)]
pub struct Bridge {
    shared: Arc<Shared>,
}

impl Bridge {
    /// Starts listening. If the socket cannot be made the bridge simply never connects (the recorder works without a stacker).
    pub fn start(path: &Path) -> Bridge {
        let shared = Arc::new(Shared { conn: Mutex::new(None), events: Mutex::new(Vec::new()), generation: AtomicU64::new(0) });
        let _ = std::fs::remove_file(path);
        match UnixListener::bind(path) {
            Ok(listener) => {
                let sh = shared.clone();
                let _ = std::thread::Builder::new().name("stack-bridge".into()).spawn(move || {
                    for stream in listener.incoming().flatten() {
                        let gen = sh.generation.fetch_add(1, Ordering::Relaxed) + 1;
                        let Ok(reader) = stream.try_clone() else { continue };
                        if let Some((_, old)) = sh.conn.lock().unwrap().replace((gen, stream)) {
                            let _ = old.shutdown(std::net::Shutdown::Both);
                        }
                        sh.events.lock().unwrap().push(Event::Connected);
                        let sh2 = sh.clone();
                        let _ = std::thread::Builder::new().name("stack-bridge-rx".into()).spawn(move || {
                            let mut rx = Receiver::new(reader);
                            while let Ok(Some(msg)) = rx.recv() {
                                if let Ok(v) = serde_json::from_slice::<Value>(&msg) {
                                    if v["t"] == "done" {
                                        sh2.events.lock().unwrap().push(Event::Done(v));
                                    }
                                }
                            }
                            let mut c = sh2.conn.lock().unwrap();
                            if c.as_ref().map(|(g, _)| *g) == Some(gen) {
                                *c = None;
                            }
                            sh2.events.lock().unwrap().push(Event::Disconnected);
                        });
                    }
                });
            }
            Err(e) => log::warn!("stack socket {}: {e}; stacking from memory is unavailable", path.display()),
        }
        Bridge { shared }
    }

    pub fn connected(&self) -> bool { self.shared.conn.lock().unwrap().is_some() }

    fn send(&self, body: &Value, fd: Option<RawFd>) -> bool {
        let c = self.shared.conn.lock().unwrap();
        match c.as_ref() {
            Some((_, s)) => ipc::send_text(s, &body.to_string(), fd).is_ok(),
            None => false,
        }
    }

    pub fn send_ring(&self, ring_no: u64, fd: RawFd) -> bool { self.send(&serde_json::json!({ "t": "ring", "ring": ring_no }), Some(fd)) }
    pub fn send_burst(&self, msg: &Value) -> bool { self.send(msg, None) }

    pub fn drain(&self) -> Vec<Event> { std::mem::take(&mut *self.shared.events.lock().unwrap()) }
}
