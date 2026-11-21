//! ZWO AM3/AM5 direct serial driver.
//!
//! Ported from Vidoxide's `mount/zwo.rs` (MIT, (c) 2024 Diego Dompe, based on
//! the ZWO serial protocol spec v1.8). Changes (ADR 0007):
//!   * guide rate is clamped to the mount's 0.1..=0.9 range instead of being
//!     pinned to 0.9 (`max` was used where `min`/`clamp` was intended);
//!     the rate is formatted with one decimal, as INDIGO does;
//!   * sky tracking rate can be selected (`:TQ#` sidereal, `:TS#` solar, `:TL#` lunar);
//!   * `stop()` no longer disables tracking (matches the `Mount` trait contract);
//!   * generic over the I/O object so the protocol is unit-testable.

use std::io::{Read, Write};
use std::time::Duration;
use rustflare_core::{Axis, Error, Mount, RadPerSec, Result, SlewSpeed, TrackingRate, SIDEREAL_RATE};

const END_CHAR: u8 = b'#';
/// Guide pulse length requested from the mount; the engine re-issues commands, so this is a safety bound.
const GUIDE_PULSE_MS: &str = "3000";

#[derive(Debug, Clone)]
enum Reply {
    None,
    EndsWith(u8),
    Exactly(&'static str),
}

struct SlewRate { id: char, mult: f64 }

/// Multiples of sidereal rate for `:R<id>#`.
const SLEW_RATES: [SlewRate; 10] = [
    SlewRate { id: '0', mult: 0.25 }, SlewRate { id: '1', mult: 0.5 }, SlewRate { id: '2', mult: 1.0 },
    SlewRate { id: '3', mult: 2.0 }, SlewRate { id: '4', mult: 4.0 }, SlewRate { id: '5', mult: 8.0 },
    SlewRate { id: '6', mult: 20.0 }, SlewRate { id: '7', mult: 60.0 }, SlewRate { id: '8', mult: 720.0 },
    SlewRate { id: '9', mult: 1440.0 },
];

fn choose_slew_rate(speed: &SlewSpeed) -> Option<&'static SlewRate> {
    match speed {
        SlewSpeed::Max(_) => SLEW_RATES.last(),
        SlewSpeed::Specific(s) => {
            let m = (s.0 / SIDEREAL_RATE.0).abs();
            SLEW_RATES.iter().find(|r| (m / r.mult - 1.0).abs() <= 0.01)
        }
    }
}

pub struct Zwo<P: Read + Write + Send> {
    port: P,
    model: String,
    device: String,
    tracking: bool,
}

impl Zwo<Box<dyn serialport::SerialPort>> {
    pub fn open(device: &str) -> Result<Self> {
        let port = serialport::new(device, 9600)
            .data_bits(serialport::DataBits::Eight)
            .flow_control(serialport::FlowControl::None)
            .parity(serialport::Parity::None)
            .stop_bits(serialport::StopBits::One)
            .timeout(Duration::from_millis(50))
            .open()
            .map_err(|e| Error::Mount(format!("opening {device}: {e}")))?;
        Zwo::new(port, device)
    }
}

impl<P: Read + Write + Send> Zwo<P> {
    pub fn new(mut port: P, device: &str) -> Result<Self> {
        let model = match send(&mut port, ":GVP#", Reply::EndsWith(END_CHAR)) {
            Ok(b) => match String::from_utf8_lossy(&b).as_ref() {
                m @ ("AM3" | "AM5") => m.to_string(),
                other => format!("unknown ({other})"),
            },
            Err(_) => "unknown (no reply)".into(),
        };
        // Start from a known state: tracking off.
        send(&mut port, ":Td#", Reply::None)?;
        Ok(Zwo { port, model, device: device.into(), tracking: false })
    }

    fn cmd(&mut self, c: &str) -> Result<()> {
        send(&mut self.port, c, Reply::None).map(|_| ())
    }
}

impl<P: Read + Write + Send> Drop for Zwo<P> {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

impl<P: Read + Write + Send> Mount for Zwo<P> {
    fn info(&self) -> String { format!("ZWO {} on {}", self.model, self.device) }

    fn set_tracking(&mut self, enabled: bool) -> Result<()> {
        send(&mut self.port, if enabled { ":Te#" } else { ":Td#" }, Reply::Exactly("1"))?;
        self.tracking = enabled;
        Ok(())
    }

    fn set_tracking_rate(&mut self, rate: TrackingRate) -> Result<()> {
        self.cmd(match rate {
            TrackingRate::Sidereal => ":TQ#",
            TrackingRate::Solar => ":TS#",
            TrackingRate::Lunar => ":TL#",
        })
    }

    fn guide(&mut self, a1: RadPerSec, a2: RadPerSec) -> Result<()> {
        let moving = !(a1.is_zero() && a2.is_zero());
        if moving && !self.tracking {
            return Err(Error::Mount("cannot guide when tracking is disabled".into()));
        }
        if a1.is_zero() {
            self.cmd(":Mge0000#")?;
            self.cmd(":Mgw0000#")?;
        }
        if a2.is_zero() {
            self.cmd(":Mgn0000#")?;
            self.cmd(":Mgs0000#")?;
        }
        if moving {
            // The mount has a single guide rate for both axes: use the larger request.
            let mult = (a1.0.abs().max(a2.0.abs()) / SIDEREAL_RATE.0).clamp(0.1, 0.9);
            self.cmd(&format!(":Rg{mult:.1}#"))?;
        }
        if a1.0 > 0.0 { self.cmd(&format!(":Mge{GUIDE_PULSE_MS}#"))?; }
        else if a1.0 < 0.0 { self.cmd(&format!(":Mgw{GUIDE_PULSE_MS}#"))?; }
        if a2.0 > 0.0 { self.cmd(&format!(":Mgn{GUIDE_PULSE_MS}#"))?; }
        else if a2.0 < 0.0 { self.cmd(&format!(":Mgs{GUIDE_PULSE_MS}#"))?; }
        Ok(())
    }

    fn slew(&mut self, axis: Axis, speed: SlewSpeed) -> Result<()> {
        if speed.is_zero() {
            return self.cmd(":Q#");
        }
        let rate = choose_slew_rate(&speed).ok_or_else(|| Error::Unsupported("slewing speed".into()))?;
        self.cmd(&format!(":R{}#", rate.id))?;
        let dir = match (axis, speed.positive()) {
            (Axis::Primary, true) => "e",
            (Axis::Primary, false) => "w",
            (Axis::Secondary, true) => "n",
            (Axis::Secondary, false) => "s",
        };
        self.cmd(&format!(":M{dir}#"))
    }

    fn slewing_speed_supported(&self, speed: RadPerSec) -> bool {
        choose_slew_rate(&SlewSpeed::Specific(speed)).is_some()
    }

    fn stop(&mut self) -> Result<()> {
        self.cmd(":Q#")
    }
}

fn send<T: Read + Write>(dev: &mut T, cmd: &str, reply: Reply) -> Result<Vec<u8>> {
    dev.write_all(cmd.as_bytes()).map_err(|e| Error::Mount(format!("write {cmd}: {e}")))?;
    let expected_len = match &reply {
        Reply::None => return Ok(vec![]),
        Reply::Exactly(s) => Some(s.len()),
        Reply::EndsWith(_) => None,
    };
    let mut buf = Vec::new();
    loop {
        let mut b = [0u8; 1];
        dev.read_exact(&mut b).map_err(|e| Error::Mount(format!("cmd {cmd}: no reply ({e})")))?;
        buf.push(b[0]);
        if buf.len() > 1024 {
            return Err(Error::Mount("reply too long".into()));
        }
        match &reply {
            Reply::EndsWith(c) if b[0] == *c => { buf.pop(); break; }
            Reply::Exactly(_) if Some(buf.len()) == expected_len => break,
            _ => {}
        }
    }
    if let Reply::Exactly(s) = reply {
        if buf != s.as_bytes() {
            return Err(Error::Mount(format!("cmd {cmd}: expected {s:?}, got {:?}", String::from_utf8_lossy(&buf))));
        }
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// Records writes, replays canned reads.
    struct Mock { written: String, replies: VecDeque<u8> }
    impl Read for Mock {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            match self.replies.pop_front() {
                Some(b) => { buf[0] = b; Ok(1) }
                None => Err(std::io::ErrorKind::TimedOut.into()),
            }
        }
    }
    impl Write for Mock {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> { self.written.push_str(&String::from_utf8_lossy(b)); Ok(b.len()) }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }

    fn mount(replies: &str) -> Zwo<Mock> {
        Zwo::new(Mock { written: String::new(), replies: replies.bytes().collect() }, "mock").unwrap()
    }

    #[test]
    fn detects_model_and_disables_tracking() {
        let m = mount("AM5#");
        assert_eq!(m.info(), "ZWO AM5 on mock");
        assert_eq!(m.port.written, ":GVP#:Td#");
    }

    #[test]
    fn guide_rate_is_clamped_not_pinned() {
        let mut m = mount("AM5#1");
        m.set_tracking(true).unwrap();
        m.port.written.clear();
        m.guide(RadPerSec(0.5 * SIDEREAL_RATE.0), RadPerSec(0.0)).unwrap();
        assert!(m.port.written.contains(":Rg0.5#"), "{}", m.port.written);
        assert!(m.port.written.contains(":Mge3000#"));
        assert!(m.port.written.contains(":Mgn0000#"));
        m.port.written.clear();
        m.guide(RadPerSec(5.0 * SIDEREAL_RATE.0), RadPerSec(0.0)).unwrap();
        assert!(m.port.written.contains(":Rg0.9#"));
    }

    #[test]
    fn guide_requires_tracking() {
        let mut m = mount("AM5#");
        assert!(m.guide(RadPerSec(0.5 * SIDEREAL_RATE.0), RadPerSec(0.0)).is_err());
    }

    #[test]
    fn solar_rate_and_slew() {
        let mut m = mount("AM5#");
        m.port.written.clear();
        m.set_tracking_rate(TrackingRate::Solar).unwrap();
        m.slew(Axis::Secondary, SlewSpeed::Specific(RadPerSec(-8.0 * SIDEREAL_RATE.0))).unwrap();
        assert_eq!(m.port.written, ":TS#:R5#:Ms#");
        assert!(!m.slewing_speed_supported(RadPerSec(3.0 * SIDEREAL_RATE.0)));
    }
}
