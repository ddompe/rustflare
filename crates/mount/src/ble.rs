//! Bluetooth Low Energy byte-stream transport for the ZWO driver (ADR 0015, `docs/zwo-ble-protocol.md`).
//!
//! The mount's BLE link is a serial-over-GATT bridge: commands are written to one characteristic, replies come
//! back as notifications on the same one. [`BleTransport`] turns that into `Read + Write` so [`crate::zwo::Zwo`]
//! runs unchanged. The platform part (BlueZ over D-Bus, feature `ble`) is behind the small [`Gatt`] trait, which
//! is also what the unit tests fake.

use rustflare_core::{Error, Result};
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(feature = "ble")]
pub mod bluez;

/// GATT service / characteristic of the control channel.
pub const SERVICE_UUID: &str = "000000ff-0000-1000-8000-00805f9b34fb";
pub const CHAR_UUID: &str = "0000ff01-0000-1000-8000-00805f9b34fb";
/// The session opener value the official app sends (a timestamp-looking constant, see the protocol notes).
/// Overridable by the settings key `zwo_ble_token` and the environment variable `RUSTFLARE_ZWO_BLE_TOKEN`.
pub const DEFAULT_TOKEN: &str = "20240222071513";
/// BLE round trips are ~60 ms but can stall; the serial reader's 50 ms is far too short.
pub const READ_TIMEOUT: Duration = Duration::from_millis(1500);
const OPENER_DEADLINE: Duration = Duration::from_secs(4);

/// A nearby BLE device that looks like a mount.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct BleDevice {
    pub address: String,
    pub name: String,
    pub rssi: Option<i16>,
}

/// Does an advertisement look like a ZWO mount: name `AM3...`/`AM5...`, or it advertises the control service.
pub fn is_mount(name: &str, service_uuids: &[String]) -> bool {
    name.starts_with("AM3") || name.starts_with("AM5") || service_uuids.iter().any(|u| u.eq_ignore_ascii_case(SERVICE_UUID))
}

/// Chooses the opener token: explicit override (env, then settings) wins over the built-in default.
/// An override that is present but empty / not all digits is an error, not a silent fallback.
pub fn resolve_token(env: Option<&str>, setting: Option<&str>) -> Result<String> {
    match env.or(setting) {
        None => Ok(DEFAULT_TOKEN.to_string()),
        Some(t) if !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()) => Ok(t.to_string()),
        Some(_) => Err(Error::Mount(
            "the configured Bluetooth session token (RUSTFLARE_ZWO_BLE_TOKEN / settings key zwo_ble_token) is empty or not digits; \
             remove it to use the built-in value, see docs/zwo-ble-protocol.md"
                .into(),
        )),
    }
}

/// What a platform GATT backend provides once connected, notifications enabled.
pub trait Gatt: Send {
    /// ATT Write Request (with response) of the whole buffer to the control characteristic.
    fn write(&mut self, data: &[u8]) -> io::Result<()>;
    /// Next notification payload, `Ok(None)` after `timeout` without one, `Err` when the link is gone.
    fn recv(&mut self, timeout: Duration) -> io::Result<Option<Vec<u8>>>;
    /// Best-effort disconnect (idempotent).
    fn disconnect(&mut self);
}

pub struct BleTransport<G: Gatt> {
    gatt: G,
    rx: VecDeque<u8>,
    timeout: Duration,
    alive: Arc<AtomicBool>,
}

impl<G: Gatt> BleTransport<G> {
    pub fn new(gatt: G, timeout: Duration) -> Self {
        BleTransport { gatt, rx: VecDeque::new(), timeout, alive: Arc::new(AtomicBool::new(true)) }
    }

    /// Shared flag that turns false when a write fails or the link drops.
    pub fn alive_flag(&self) -> Arc<AtomicBool> { self.alive.clone() }

    /// `:BS<token>#` -> `OK#`; the mount ignores everything else until this succeeded.
    pub fn open_session(&mut self, token: &str) -> Result<()> {
        if token.is_empty() || !token.bytes().all(|b| b.is_ascii_digit()) {
            return Err(Error::Mount("invalid Bluetooth session token, see docs/zwo-ble-protocol.md".into()));
        }
        self.write_all(format!(":BS{token}#").as_bytes()).map_err(|e| Error::Mount(format!("Bluetooth session opener: {e}")))?;
        let start = Instant::now();
        let mut reply = Vec::new();
        while start.elapsed() < OPENER_DEADLINE {
            let mut b = [0u8; 1];
            match self.read(&mut b) {
                Ok(1) => {
                    reply.push(b[0]);
                    if b[0] == b'#' {
                        return if reply == b"OK#" {
                            Ok(())
                        } else {
                            Err(Error::Mount(format!("Bluetooth session refused: {:?}", String::from_utf8_lossy(&reply))))
                        };
                    }
                }
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::TimedOut => break,
                Err(e) => return Err(Error::Mount(format!("Bluetooth session opener: {e}"))),
            }
        }
        Err(Error::Mount("the mount did not answer the Bluetooth session opener (is the phone app closed?)".into()))
    }

    fn dead(&self) -> io::Error { io::Error::new(io::ErrorKind::BrokenPipe, "Bluetooth link lost") }
}

impl<G: Gatt> Read for BleTransport<G> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if self.rx.is_empty() {
            if !self.alive.load(Ordering::Relaxed) {
                return Err(self.dead());
            }
            match self.gatt.recv(self.timeout) {
                Ok(Some(v)) => self.rx.extend(v),
                Ok(None) => return Err(io::ErrorKind::TimedOut.into()),
                Err(e) => {
                    self.alive.store(false, Ordering::Relaxed);
                    return Err(e);
                }
            }
        }
        let mut n = 0;
        while n < buf.len() {
            match self.rx.pop_front() {
                Some(b) => { buf[n] = b; n += 1; }
                None => break,
            }
        }
        Ok(n)
    }
}

impl<G: Gatt> Write for BleTransport<G> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if !self.alive.load(Ordering::Relaxed) {
            return Err(self.dead());
        }
        // Whatever is still queued is a late reply to an earlier command: drop it so replies cannot be mismatched.
        self.rx.clear();
        loop {
            match self.gatt.recv(Duration::ZERO) {
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(e) => {
                    self.alive.store(false, Ordering::Relaxed);
                    return Err(e);
                }
            }
        }
        if let Err(e) = self.gatt.write(data) {
            self.alive.store(false, Ordering::Relaxed);
            return Err(e);
        }
        Ok(data.len())
    }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}

impl<G: Gatt> Drop for BleTransport<G> {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
        self.gatt.disconnect();
    }
}

/// Opens the BLE link to the mount at `address` and starts the session. Blocking; bounded by timeouts.
#[cfg(feature = "ble")]
pub fn open(address: &str, token: &str) -> Result<(BleTransport<bluez::BluezGatt>, Arc<AtomicBool>)> {
    let gatt = bluez::BluezGatt::connect(address, Duration::from_secs(20))?;
    let mut t = BleTransport::new(gatt, READ_TIMEOUT);
    t.open_session(token)?;
    let alive = t.alive_flag();
    Ok((t, alive))
}

#[cfg(not(feature = "ble"))]
pub fn open(_address: &str, _token: &str) -> Result<(BleTransport<NoBackend>, Arc<AtomicBool>)> {
    Err(Error::Unsupported("this build has no Bluetooth support (cargo feature `ble`)".into()))
}

/// Nearby mounts. Blocking for about `duration`.
#[cfg(feature = "ble")]
pub fn scan(duration: Duration) -> Result<Vec<BleDevice>> { bluez::scan(duration) }

#[cfg(not(feature = "ble"))]
pub fn scan(_duration: Duration) -> Result<Vec<BleDevice>> {
    Err(Error::Unsupported("this build has no Bluetooth support (cargo feature `ble`)".into()))
}

/// Placeholder type so the non-`ble` signature of [`open`] is well-formed.
#[cfg(not(feature = "ble"))]
pub struct NoBackend;
#[cfg(not(feature = "ble"))]
impl Gatt for NoBackend {
    fn write(&mut self, _: &[u8]) -> io::Result<()> { Err(io::ErrorKind::Unsupported.into()) }
    fn recv(&mut self, _: Duration) -> io::Result<Option<Vec<u8>>> { Err(io::ErrorKind::Unsupported.into()) }
    fn disconnect(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{channel, Receiver, Sender};
    use std::sync::Mutex;

    /// Fake backend: records writes, hands out scripted notifications; `answer` maps a write to notifications.
    struct Fake {
        written: Arc<Mutex<Vec<Vec<u8>>>>,
        rx: Receiver<io::Result<Vec<u8>>>,
        tx: Sender<io::Result<Vec<u8>>>,
        answer: Box<dyn FnMut(&[u8]) -> Vec<Vec<u8>> + Send>,
        fail_writes: bool,
        disconnected: Arc<AtomicBool>,
    }
    impl Gatt for Fake {
        fn write(&mut self, d: &[u8]) -> io::Result<()> {
            if self.fail_writes {
                return Err(io::Error::new(io::ErrorKind::Other, "att error"));
            }
            self.written.lock().unwrap().push(d.to_vec());
            for r in (self.answer)(d) { self.tx.send(Ok(r)).unwrap(); }
            Ok(())
        }
        fn recv(&mut self, t: Duration) -> io::Result<Option<Vec<u8>>> {
            match self.rx.recv_timeout(t.min(Duration::from_millis(30))) {
                Ok(r) => r.map(Some),
                Err(_) => Ok(None),
            }
        }
        fn disconnect(&mut self) { self.disconnected.store(true, Ordering::SeqCst); }
    }
    fn fake(answer: impl FnMut(&[u8]) -> Vec<Vec<u8>> + Send + 'static) -> (Fake, Arc<Mutex<Vec<Vec<u8>>>>, Arc<AtomicBool>) {
        let (tx, rx) = channel();
        let written = Arc::new(Mutex::new(vec![]));
        let disc = Arc::new(AtomicBool::new(false));
        (Fake { written: written.clone(), rx, tx, answer: Box::new(answer), fail_writes: false, disconnected: disc.clone() }, written, disc)
    }
    const FAKE_TOKEN: &str = "00000000000000";

    #[test]
    fn opener_succeeds_and_is_one_write() {
        let (g, w, _) = fake(|d| if d.starts_with(b":BS") { vec![b"OK#".to_vec()] } else { vec![] });
        let mut t = BleTransport::new(g, Duration::from_millis(100));
        t.open_session(FAKE_TOKEN).unwrap();
        assert_eq!(w.lock().unwrap()[0], b":BS00000000000000#");
    }

    #[test]
    fn opener_failure_modes() {
        // refused
        let (g, _, _) = fake(|_| vec![b"0#".to_vec()]);
        assert!(BleTransport::new(g, Duration::from_millis(100)).open_session(FAKE_TOKEN).unwrap_err().to_string().contains("refused"));
        // silence
        let (g, _, _) = fake(|_| vec![]);
        assert!(BleTransport::new(g, Duration::from_millis(60)).open_session(FAKE_TOKEN).unwrap_err().to_string().contains("did not answer"));
        // bad token never reaches the radio
        let (g, w, _) = fake(|_| vec![]);
        assert!(BleTransport::new(g, Duration::from_millis(60)).open_session("").is_err());
        assert!(w.lock().unwrap().is_empty());
    }

    #[test]
    fn chunked_notifications_are_reassembled_by_byte_reads() {
        let (g, _, _) = fake(|_| vec![b"11/21/26&15:2".to_vec(), b"0:38&+06:00#01:47".to_vec(), b":36&+90*00:00#nN#".to_vec()]);
        let mut t = BleTransport::new(g, Duration::from_millis(100));
        t.write_all(b":GAST#").unwrap();
        let mut out = Vec::new();
        let mut hashes = 0;
        while hashes < 3 {
            let mut b = [0u8; 1];
            t.read_exact(&mut b).unwrap();
            out.push(b[0]);
            if b[0] == b'#' { hashes += 1; }
        }
        assert_eq!(String::from_utf8(out).unwrap(), "11/21/26&15:20:38&+06:00#01:47:36&+90*00:00#nN#");
    }

    #[test]
    fn read_times_out_without_notification() {
        let (g, _, _) = fake(|_| vec![]);
        let mut t = BleTransport::new(g, Duration::from_millis(40));
        let e = t.read(&mut [0u8; 1]).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::TimedOut);
        assert!(t.alive_flag().load(Ordering::Relaxed), "a timeout alone is not a lost link");
    }

    #[test]
    fn stale_replies_are_dropped_before_the_next_command() {
        let (g, _, _) = fake(|d| if d == b":GU#" { vec![b"nN#".to_vec()] } else { vec![] });
        let mut t = BleTransport::new(g, Duration::from_millis(100));
        t.write_all(b":GU#").unwrap(); // reply arrives but is never read
        t.write_all(b":GU#").unwrap(); // the old one must not be read as the answer to this one
        let mut buf = [0u8; 16];
        let n = t.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"nN#");
        assert_eq!(t.read(&mut buf).unwrap_err().kind(), io::ErrorKind::TimedOut);
    }

    #[test]
    fn write_failure_marks_the_link_lost_and_stays_lost() {
        let (mut g, _, _) = fake(|_| vec![]);
        g.fail_writes = true;
        let mut t = BleTransport::new(g, Duration::from_millis(40));
        let alive = t.alive_flag();
        assert!(t.write_all(b":Q#").is_err());
        assert!(!alive.load(Ordering::Relaxed));
        assert_eq!(t.write(b":Q#").unwrap_err().kind(), io::ErrorKind::BrokenPipe);
        assert_eq!(t.read(&mut [0u8; 1]).unwrap_err().kind(), io::ErrorKind::BrokenPipe);
    }

    #[test]
    fn link_drop_reported_by_backend_surfaces_on_read_and_drop_disconnects() {
        let (g, _, disc) = fake(|_| vec![]);
        g.tx.send(Err(io::Error::new(io::ErrorKind::ConnectionAborted, "disconnected"))).unwrap();
        let mut t = BleTransport::new(g, Duration::from_millis(100));
        let alive = t.alive_flag();
        assert_eq!(t.read(&mut [0u8; 1]).unwrap_err().kind(), io::ErrorKind::ConnectionAborted);
        assert!(!alive.load(Ordering::Relaxed));
        drop(t);
        assert!(disc.load(Ordering::SeqCst));
    }

    #[test]
    fn zwo_driver_runs_on_the_transport() {
        use crate::zwo::Zwo;
        use rustflare_core::Mount;
        let (g, w, _) = fake(|d| match d {
            b":GVP#" => vec![b"AM3#".to_vec()],
            b":GU#" => vec![b"nN".to_vec(), b"#".to_vec()],
            b":Gps#" => vec![b"0#".to_vec()],
            _ => vec![],
        });
        let t = BleTransport::new(g, Duration::from_millis(100));
        let alive = t.alive_flag();
        let mut z = Zwo::with_link(t, "ble", "ble", Some(alive.clone())).unwrap();
        assert_eq!(z.link().unwrap().kind, "ble");
        assert!(z.link().unwrap().up);
        let st = z.state().unwrap();
        assert_eq!(st.tracking, Some(false));
        drop(z); // Drop sends :Q# (stop) before the link is closed
        assert_eq!(w.lock().unwrap().last().unwrap(), b":Q#");
    }

    #[test]
    fn token_resolution() {
        assert_eq!(resolve_token(None, None).unwrap(), DEFAULT_TOKEN);
        assert_eq!(resolve_token(Some("123"), Some("456")).unwrap(), "123");
        assert_eq!(resolve_token(None, Some("456")).unwrap(), "456");
        assert!(resolve_token(Some(""), None).unwrap_err().to_string().contains("zwo-ble-protocol.md"));
        assert!(resolve_token(None, Some("12x")).is_err());
    }

    #[test]
    fn mount_recognition() {
        assert!(is_mount("AM3_Something", &[]));
        assert!(is_mount("AM5N", &[]));
        assert!(is_mount("x", &["000000FF-0000-1000-8000-00805F9B34FB".into()]));
        assert!(!is_mount("JBL Flip", &["0000180f-0000-1000-8000-00805f9b34fb".into()]));
    }
}
