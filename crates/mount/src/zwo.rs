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

use rustflare_core::{Axis, Error, LinkInfo, Mount, MountState, ParkState, RadPerSec, Result, SiteTime, SlewSpeed, TrackingRate, SIDEREAL_RATE};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const END_CHAR: u8 = b'#';
/// Guide pulse length requested from the mount; the engine re-issues commands, so this is a safety bound.
const GUIDE_PULSE_MS: &str = "3000";

#[derive(Debug, Clone)]
enum Reply {
    None,
    EndsWith(u8),
    /// The reply has `n` segments, each ended by the terminator: read until the n-th (`:GAST#` answers `date#pointing#flags#`).
    Segments(u8, usize),
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
    /// Transport kind (`usb`/`ble`) and, for links that can drop, a flag the transport clears when it is lost.
    link_kind: &'static str,
    link_alive: Option<Arc<AtomicBool>>,
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
    pub fn new(port: P, device: &str) -> Result<Self> {
        Self::with_link(port, device, "usb", None)
    }

    pub fn with_link(mut port: P, device: &str, link_kind: &'static str, link_alive: Option<Arc<AtomicBool>>) -> Result<Self> {
        let model = match send(&mut port, ":GVP#", Reply::EndsWith(END_CHAR)) {
            Ok(b) => match String::from_utf8_lossy(&b).as_ref() {
                m @ ("AM3" | "AM5") => m.to_string(),
                other => format!("unknown ({other})"),
            },
            Err(_) => "unknown (no reply)".into(),
        };
        // Do NOT force a state: the mount may already be tracking (a service restart or update must not stop
        // it), or have been changed with its own buttons. Ask it. (The first version sent `:Td#` here.)
        let tracking = match send(&mut port, ":GU#", Reply::EndsWith(END_CHAR)) {
            Ok(flags) => !flags.contains(&b'n'),
            Err(_) => false,
        };
        Ok(Zwo { port, model, device: device.into(), tracking, link_kind, link_alive })
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
    fn link(&self) -> Option<LinkInfo> {
        Some(LinkInfo { kind: self.link_kind, up: self.link_alive.as_ref().map_or(true, |a| a.load(Ordering::Relaxed)) })
    }

    fn info(&self) -> String { format!("ZWO {} on {}", self.model, self.device) }

    /// `:GU#` flags: `n` = tracking OFF, `N` = not slewing, `H` = at home, `Z` = alt-az mode; `:Gps#` = park state
    /// (0 not parked, 1 parking, 2 parked; read-only here). A parked mount reports no `N`, which is not a slew.
    fn state(&mut self) -> Result<MountState> {
        let flags = send(&mut self.port, ":GU#", Reply::EndsWith(END_CHAR))?;
        let has = |c: u8| flags.contains(&c);
        let park = send(&mut self.port, ":Gps#", Reply::EndsWith(END_CHAR)).ok().and_then(|r| match r.first() {
            Some(b'0') => Some(ParkState::Unparked),
            Some(b'1') => Some(ParkState::Parking),
            Some(b'2') => Some(ParkState::Parked),
            _ => None,
        });
        let parked = park == Some(ParkState::Parked);
        let slewing = !has(b'N') && !parked;
        let at_home = has(b'H');
        self.tracking = !has(b'n');

        Ok(MountState { tracking: Some(self.tracking), slewing: Some(slewing), at_home: Some(at_home), park, alt_az: Some(has(b'Z')) })
    }

    /// Pointing from `:GAST#`, whose reply is `<date&time&offset>#<RA&Dec>#<flags>#` (read-only query).
    fn equatorial(&mut self) -> Result<Option<(f64, f64)>> {
        let r = send(&mut self.port, ":GAST#", Reply::Segments(END_CHAR, 3))?;
        Ok(parse_gast_pointing(&String::from_utf8_lossy(&r)))
    }

    /// `:SMGE<lat>&<lon>#` and `:SMTI<MM/DD/YY>&<hh:mm:ss>&<offset>#`, as the official app sends them. Longitude is
    /// west-positive; the UTC offset is sent with the opposite sign (a site at UTC-6 as `+06:00`).
    fn set_site_time(&mut self, st: &SiteTime) -> Result<()> {
        send(&mut self.port, &format!(":SMGE{}&{}#", fmt_dms(st.latitude_deg, 2), fmt_dms(-st.longitude_east_deg, 3)), Reply::Exactly("1"))?;
        let (y, mo, d, h, mi, s) = civil(st.unix_secs + st.utc_offset_minutes as i64 * 60);
        let off = -st.utc_offset_minutes;
        let time = format!("{mo:02}/{d:02}/{:02}&{h:02}:{mi:02}:{s:02}&{}{:02}:{:02}", y.rem_euclid(100), if off >= 0 { '+' } else { '-' }, off.abs() / 60, off.abs() % 60);
        send(&mut self.port, &format!(":SMTI{time}#"), Reply::Exactly("1")).map(|_| ())
    }

    /// LX200 GoTo as INDIGO does it: `:Sr<hh:mm:ss>#` -> `1`, `:Sd<sdd*mm:ss>#` -> `1`, `:MS#` -> `0` (slewing).
    fn goto_equatorial(&mut self, ra_hours: f64, dec_deg: f64) -> Result<()> {
        let secs = (ra_hours.rem_euclid(24.0) * 3600.0).round() as u32 % 86400;
        send(&mut self.port, &format!(":Sr{:02}:{:02}:{:02}#", secs / 3600, secs % 3600 / 60, secs % 60), Reply::Exactly("1"))?;
        send(&mut self.port, &format!(":Sd{}#", fmt_dms(dec_deg, 2)), Reply::Exactly("1"))?;
        send(&mut self.port, ":MS#", Reply::Exactly("0")).map_err(|e| Error::Mount(format!("the mount refused the GoTo: {e}")))?;
        self.tracking = false; // a GoTo ends with the mount stopped until tracking is switched on again
        Ok(())
    }

    fn go_home(&mut self) -> Result<()> {
        self.cmd(":hC#")?;
        self.tracking = false;
        Ok(())
    }

    fn set_tracking(&mut self, enabled: bool) -> Result<()> {
        if let Err(e) = send(&mut self.port, if enabled { ":Te#" } else { ":Td#" }, Reply::Exactly("1")) {
            // the mount answers 0 when it will not: say why instead of "expected 1, got 0"
            return Err(match (&e, enabled) {
                (Error::Mount(m), true) if m.contains("got \"0\"") => {
                    let why = self.why_not_tracking().unwrap_or_else(|| "it gave no reason".into());
                    Error::Mount(format!("the mount would not start tracking: {why}"))
                }
                (Error::Mount(m), false) if m.contains("got \"0\"") => Error::Mount("the mount would not stop tracking: it is slewing or going to its home position (press STOP first)".into()),
                _ => e,
            });
        }
        self.tracking = enabled;
        Ok(())
    }

    fn query(&mut self, what: &str) -> Result<String> {
        // read-only getters only (the protocol's `G` commands that take no argument)
        const ALLOWED: [&str; 12] = ["GAT", "GTa", "GT", "GU", "Gps", "GVP", "GLC", "GLH", "GLL", "GR", "GD", "GAST"];
        if !ALLOWED.contains(&what) {
            return Err(Error::Unsupported(format!("query {what}: allowed are {}", ALLOWED.join(", "))));
        }
        let cmd = format!(":{what}#");
        let reply = if what == "GAST" { send(&mut self.port, &cmd, Reply::Segments(b'#', 3)) } else { send(&mut self.port, &cmd, Reply::EndsWith(END_CHAR)) }?;
        Ok(String::from_utf8_lossy(&reply).to_string())
    }

    fn why_not_tracking(&mut self) -> Option<String> {
        let r = self.query("GAT").ok()?;
        Some(explain_tracking_status(&r))
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

/// The RA/Dec part of a `:GAST#` reply: `11/21/26&15:20:38&+06:00#01:47:36&+90*00:00#nNHGM040400880#`.
/// `:GAT#` answers `0` (tracking off, no fault), `1` (tracking) or `e<code>` (it stopped, or would not start, and why).
pub fn explain_tracking_status(reply: &str) -> String {
    let r = reply.trim().trim_end_matches('#');
    match r {
        "1" => "it is tracking".into(),
        "0" => "tracking is off and the mount reports no fault (it was switched off, or it is at its home position)".into(),
        "e3" => "it is going home, slewing or doing a Go to".into(),
        "e4" => "it is moving".into(),
        "e5" => "the target is below the horizon".into(),
        "e6" => "the target is below the mount's altitude limit".into(),
        "e7" => "the mount's time and place are not set: do a Go to (it sends them) or set the site".into(),
        "e8" => "the mount has passed its meridian limit while tracking: flip the mount (Aim > Meridian flip), or change the meridian setting in the mount's own app".into(),
        other => format!("the mount reported {other} (see the ZWO protocol's error codes)"),
    }
}

fn parse_gast_pointing(reply: &str) -> Option<(f64, f64)> {
    let part = reply.split('#').nth(1)?;
    let (ra, dec) = part.split_once('&')?;
    let mut r = ra.split(':').map(|v| v.trim().parse::<f64>());
    let ra_h = r.next()?.ok()? + r.next()?.ok()? / 60.0 + r.next()?.ok()? / 3600.0;
    let sign = if dec.trim_start().starts_with('-') { -1.0 } else { 1.0 };
    let d = dec.trim_start_matches(['+', '-']);
    let (deg, rest) = d.split_once('*')?;
    let (min, sec) = rest.split_once(':')?;
    Some((ra_h, sign * (deg.parse::<f64>().ok()? + min.parse::<f64>().ok()? / 60.0 + sec.parse::<f64>().ok()? / 3600.0)))
}

/// `±dd*mm:ss` (`deg_width` digits of degrees), rounded to the nearest arcsecond.
fn fmt_dms(deg: f64, deg_width: usize) -> String {
    let total = (deg.abs() * 3600.0).round() as u32;
    format!("{}{:0w$}*{:02}:{:02}", if deg < 0.0 { '-' } else { '+' }, total / 3600, total % 3600 / 60, total % 60, w = deg_width)
}

/// Civil date and time (UTC arithmetic) of a Unix timestamp: (year, month, day, hour, minute, second).
fn civil(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400) as u32;
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d, rem / 3600, rem % 3600 / 60, rem % 60)
}

fn send<T: Read + Write>(dev: &mut T, cmd: &str, reply: Reply) -> Result<Vec<u8>> {
    let started = std::time::Instant::now();
    let r = send_raw(dev, cmd, reply);
    match &r {
        Ok(b) => crate::trace::record(cmd, Ok(&String::from_utf8_lossy(b)), started),
        Err(e) => crate::trace::record(cmd, Err(&e.to_string()), started),
    }
    r
}

fn send_raw<T: Read + Write>(dev: &mut T, cmd: &str, reply: Reply) -> Result<Vec<u8>> {
    dev.write_all(cmd.as_bytes()).map_err(|e| Error::Mount(format!("write {cmd}: {e}")))?;
    let expected_len = match &reply {
        Reply::None => return Ok(vec![]),
        Reply::Exactly(s) => Some(s.len()),
        Reply::EndsWith(_) | Reply::Segments(..) => None,
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
            Reply::Segments(c, n) if b[0] == *c && buf.iter().filter(|x| **x == *c).count() >= *n => break,
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

    /// Model reply and a "tracking off" `:GU#` reply (what the constructor reads), then `extra` replies.
    fn mount(extra: &str) -> Zwo<Mock> {
        Zwo::new(Mock { written: String::new(), replies: format!("AM5#n#{extra}").bytes().collect() }, "mock").unwrap()
    }

    #[test]
    fn pointing_is_read_from_the_whole_three_part_gast_reply() {
        // the mount answers :GAST# with three '#'-terminated parts; reading only the first one lost the pointing (found on the real mount)
        let reply = "11/22/26&11:42:07&+06:00#15:47:53&-19*58:26#nNHGM040400880#";
        let mut m = Zwo::new(Mock { written: String::new(), replies: format!("AM5#n#{reply}").bytes().collect() }, "mock").unwrap();
        let (ra, dec) = m.equatorial().unwrap().expect("pointing");
        assert!((ra - 15.7981).abs() < 0.001 && (dec + 19.974).abs() < 0.001, "{ra} {dec}");
        assert!(m.port.replies.is_empty(), "the whole reply must be consumed, or it corrupts the next answer");
    }

    #[test]
    fn detects_model_and_reads_the_tracking_state_without_changing_it() {
        let m = mount("");
        assert_eq!(m.info(), "ZWO AM5 on mock");
        // it asks the mount for its state; it never forces tracking off
        assert_eq!(m.port.written, ":GVP#:GU#");
        assert!(!m.tracking);
    }

    #[test]
    fn guide_rate_is_clamped_not_pinned() {
        let mut m = mount("1");
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
        let mut m = mount("");
        assert!(m.guide(RadPerSec(0.5 * SIDEREAL_RATE.0), RadPerSec(0.0)).is_err());
    }

    #[test]
    fn solar_rate_and_slew() {
        let mut m = mount("");
        m.port.written.clear();
        m.set_tracking_rate(TrackingRate::Solar).unwrap();
        m.slew(Axis::Secondary, SlewSpeed::Specific(RadPerSec(-8.0 * SIDEREAL_RATE.0))).unwrap();
        assert_eq!(m.port.written, ":TS#:R5#:Ms#");
        assert!(!m.slewing_speed_supported(RadPerSec(3.0 * SIDEREAL_RATE.0)));
    }

    #[test]
    fn already_tracking_mount_is_left_tracking_on_connect() {
        // `:GU#` without the `n` flag = tracking ON (a service restart must not stop the mount)
        let m = Zwo::new(Mock { written: String::new(), replies: "AM5#N#".bytes().collect() }, "mock").unwrap();
        assert!(m.tracking);
        assert!(!m.port.written.contains(":Td#"));
    }

    #[test]
    fn state_reads_external_changes() {
        let mut m = mount("N#0#");
        let s = m.state().unwrap();
        assert_eq!((s.tracking, s.slewing, s.park), (Some(true), Some(false), Some(ParkState::Unparked)));
        // parked with the mount's own button: tracking off, no `N`, park state 2 — not "slewing"
        let mut m = mount("n#2#");
        let s = m.state().unwrap();
        assert_eq!((s.tracking, s.slewing, s.park), (Some(false), Some(false), Some(ParkState::Parked)));
        let mut m = mount("nZ#0#");
        assert_eq!(m.state().unwrap().alt_az, Some(true));
    }

    #[test]
    fn go_home_sends_the_home_command() {
        let mut m = mount("");
        m.port.written.clear();
        m.go_home().unwrap();
        assert_eq!(m.port.written, ":hC#");
        assert!(!m.tracking);
    }

    #[test]
    fn site_and_time_use_the_official_app_format() {
        // site 40.5 N, 105.25 W; 2026-11-21 21:20:38 UTC, local time UTC-6 => 15:20:38 and offset sent as +06:00
        let mut m = mount("11");
        m.port.written.clear();
        let st = SiteTime { latitude_deg: 40.5, longitude_east_deg: -105.25, unix_secs: 1_795_296_038, utc_offset_minutes: -360 };
        m.set_site_time(&st).unwrap();
        assert_eq!(m.port.written, ":SMGE+40*30:00&+105*15:00#:SMTI11/21/26&15:20:38&+06:00#");
    }

    #[test]
    fn goto_sends_target_then_slew_and_clears_the_tracking_flag() {
        let mut m = mount("110");
        m.port.written.clear();
        m.goto_equatorial(15.7981, -19.974).unwrap();
        assert_eq!(m.port.written, ":Sr15:47:53#:Sd-19*58:26#:MS#");
        assert!(!m.tracking);
        // a refused slew (anything but 0) is an error
        let mut m = mount("11e4");
        assert!(m.goto_equatorial(1.0, 1.0).is_err());
    }

    #[test]
    fn civil_dates_are_right() {
        assert_eq!(civil(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(civil(1_795_296_038), (2026, 11, 21, 21, 20, 38));
        assert_eq!(fmt_dms(-0.5, 2), "-00*30:00");
    }

    #[test]
    fn a_refused_tracking_start_says_why() {
        assert!(explain_tracking_status("e8#").contains("meridian limit"));
        assert!(explain_tracking_status("e5").contains("horizon"));
        assert!(explain_tracking_status("0#").contains("no fault"));
        assert!(explain_tracking_status("e99#").contains("e99"));
        // end to end on a scripted port: :Te# answers 0, then :GAT# answers e8
        let mut m = mount("0e8#");
        let e = m.set_tracking(true).unwrap_err().to_string();
        assert!(e.contains("would not start tracking") && e.contains("meridian limit"), "{e}");
        assert!(m.port.written.ends_with(":Te#:GAT#"), "{}", m.port.written);
        // only getters can be asked for
        let mut m = mount("1#");
        assert!(m.query(":Te").is_err() && m.query("SLD").is_err());
        assert_eq!(m.query("GAT").unwrap(), "1");
    }

    #[test]
    fn pointing_is_parsed_from_the_gast_reply() {
        let (ra, dec) = parse_gast_pointing("11/21/26&15:20:38&+06:00#01:47:36&+90*00:00#nNHGM040400880#").unwrap();
        assert!((ra - (1.0 + 47.0 / 60.0 + 36.0 / 3600.0)).abs() < 1e-9 && (dec - 90.0).abs() < 1e-9);
        let (_, d) = parse_gast_pointing("x#15:47:53&-19*58:26#n#").unwrap();
        assert!((d + (19.0 + 58.0 / 60.0 + 26.0 / 3600.0)).abs() < 1e-9);
        assert_eq!(parse_gast_pointing("garbage"), None);
    }
}
