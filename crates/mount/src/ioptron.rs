//! iOptron mounts over the RS-232 command language 2014 version 3 (CEM70 series, CEM120, CEM40, GEM45, CEM26, GEM28, and the
//! HAE69 family that AsiAir drives as a CEM70). USB mounts show up as a serial port; the settings are 115200 8N1.
//!
//! Source: "iOptron Mount RS-232 Command Language, version 3.10" (2021-01-04). **Written from that document, tested against a mock
//! only: no iOptron mount has been connected yet** (see docs/project/hardware-checklist.md).
//!
//! What the protocol looks like: ASCII, every command `:...#`; a set command answers a single `1` (or `0` when it refuses) with no
//! terminator; a get command answers its data and a closing `#`; `:MountInfo#` answers four digits; the motion commands `:m?#`, `:Z?nnnnn#`
//! and `:M?nnnnn#` have no answer.

use rustflare_core::{Axis, Error, LinkInfo, Mount, MountState, ParkState, RadPerSec, Result, SiteTime, SlewSpeed, TrackingRate, SIDEREAL_RATE};
use std::io::{Read, Write};
use std::time::Duration;

/// Arc-seconds in 0.01 arc-second units: the protocol's coordinate unit (360 degrees = 129,600,000).
const CENTI_ARCSEC_PER_DEG: f64 = 360_000.0;
const FULL_CIRCLE: f64 = 129_600_000.0;
/// Unix time of J2000.0 (2000-01-01 12:00 UTC) in milliseconds: `:SUT` counts milliseconds since then.
const J2000_UNIX_MS: i64 = 946_728_000_000;
/// The longest a guide pulse is asked for; the engine re-issues, so this is only a safety bound (the protocol allows 99,999 ms).
const GUIDE_PULSE_MS: u32 = 2_500;

#[derive(Debug, Clone, Copy)]
enum Reply {
    /// Motion commands: no answer.
    None,
    /// A set command: one character, `1` accepted.
    One,
    /// Read until `#` (not included).
    Hash,
    /// A fixed number of bytes (`:MountInfo#` answers four digits).
    Fixed(usize),
}

/// Moving rates for `:SRn#`, as multiples of sidereal.
const MOVE_RATES: [(u8, f64); 8] = [(1, 1.0), (2, 2.0), (3, 8.0), (4, 16.0), (5, 64.0), (6, 128.0), (7, 256.0), (8, 512.0)];

fn move_rate(speed: &SlewSpeed) -> Option<u8> {
    match speed {
        SlewSpeed::Max(_) => Some(9),
        SlewSpeed::Specific(s) => {
            let m = (s.0 / SIDEREAL_RATE.0).abs();
            MOVE_RATES.iter().find(|(_, mult)| (m / mult - 1.0).abs() <= 0.01).map(|(n, _)| *n)
        }
    }
}

fn model_name(code: &str) -> String {
    match code {
        "0026" => "CEM26", "0027" => "CEM26-EC", "0028" => "GEM28", "0029" => "GEM28-EC", "0040" => "CEM40", "0041" => "CEM40-EC", "0043" => "GEM45", "0044" => "GEM45-EC",
        "0070" => "CEM70", "0071" => "CEM70-EC", "0120" => "CEM120", "0121" => "CEM120-EC", "0122" => "CEM120-EC2",
        other => return format!("model {other}"),
    }
    .into()
}

pub struct Ioptron<P: Read + Write + Send> {
    port: P,
    model: String,
    device: String,
    tracking: bool,
    /// Guiding rates last set (RA, Dec, in hundredths of sidereal), so they are only sent when they change.
    guide_rates: Option<(u8, u8)>,
}

impl Ioptron<Box<dyn serialport::SerialPort>> {
    pub fn open(device: &str) -> Result<Self> {
        let port = serialport::new(device, 115_200)
            .data_bits(serialport::DataBits::Eight)
            .flow_control(serialport::FlowControl::None)
            .parity(serialport::Parity::None)
            .stop_bits(serialport::StopBits::One)
            .timeout(Duration::from_millis(400))
            .open()
            .map_err(|e| Error::Mount(format!("opening {device}: {e}")))?;
        Ioptron::new(port, device)
    }
}

impl<P: Read + Write + Send> Ioptron<P> {
    /// Sends `:MountInfo#` first, as the protocol document says every program must, and fails if nothing answers (wrong port or baud rate).
    pub fn new(mut port: P, device: &str) -> Result<Self> {
        let info = send(&mut port, ":MountInfo#", Reply::Fixed(4)).map_err(|e| Error::Mount(format!("{e}: is this the iOptron mount's serial port (115200 baud)?")))?;
        let code = String::from_utf8_lossy(&info).to_string();
        if code.len() != 4 || !code.bytes().all(|b| b.is_ascii_digit()) {
            return Err(Error::Mount(format!("an iOptron mount answers :MountInfo# with four digits, this answered {code:?}")));
        }
        let mut m = Ioptron { port, model: model_name(&code), device: device.into(), tracking: false, guide_rates: None };
        // do not force a state: ask what the mount is doing now
        if let Ok(s) = m.state() {
            m.tracking = s.tracking.unwrap_or(false);
        }
        Ok(m)
    }

    fn set(&mut self, c: &str) -> Result<()> {
        send(&mut self.port, c, Reply::One).map(|_| ())
    }

    fn cmd(&mut self, c: &str) -> Result<()> {
        send(&mut self.port, c, Reply::None).map(|_| ())
    }

    /// The 22 digits of `:GLS#` after the sign: longitude, latitude + 90, gps, system status, tracking rate, move speed, time source, hemisphere.
    fn gls(&mut self) -> Result<Vec<u8>> {
        let r = send(&mut self.port, ":GLS#", Reply::Hash)?;
        let digits: Vec<u8> = r.iter().skip(1).copied().collect();
        if r.len() != 23 || !digits.iter().all(u8::is_ascii_digit) {
            return Err(Error::Mount(format!(":GLS# answered {:?}, expected a sign and 22 digits", String::from_utf8_lossy(&r))));
        }
        Ok(digits)
    }

    fn status_digit(&mut self) -> Result<u8> {
        Ok(self.gls()?[17] - b'0')
    }
}

impl<P: Read + Write + Send> Drop for Ioptron<P> {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

impl<P: Read + Write + Send> Mount for Ioptron<P> {
    fn link(&self) -> Option<LinkInfo> {
        Some(LinkInfo { kind: "usb", up: true })
    }

    fn info(&self) -> String { format!("iOptron {} on {}", self.model, self.device) }

    /// System status (the 18th digit of `:GLS#`): 0 stopped away from zero, 1 tracking, 2 slewing, 3 guiding, 4 meridian flip,
    /// 5 tracking with periodic error correction, 6 parked, 7 stopped at zero (home).
    fn state(&mut self) -> Result<MountState> {
        let s = self.status_digit()?;
        self.tracking = matches!(s, 1 | 3 | 5);
        Ok(MountState {
            tracking: Some(self.tracking),
            slewing: Some(matches!(s, 2 | 4)),
            at_home: Some(s == 7),
            park: Some(if s == 6 { ParkState::Parked } else { ParkState::Unparked }),
            alt_az: None,
        })
    }

    /// `:GEP#`: sign, 8 digits of declination, 9 of right ascension (both in 0.01 arc-seconds), side of pier, pointing state.
    fn equatorial(&mut self) -> Result<Option<(f64, f64)>> {
        let r = send(&mut self.port, ":GEP#", Reply::Hash)?;
        Ok(parse_gep(&String::from_utf8_lossy(&r)))
    }

    /// `:SGsMMM#` time zone (minutes east of UTC), `:SDS0#` no daylight saving (the offset already includes it), `:SUT<ms since J2000>#`,
    /// `:SLOsTTTTTTTT#` longitude (east positive), `:SLAsTTTTTTTT#` latitude, `:SHE0/1#` hemisphere.
    fn set_site_time(&mut self, st: &SiteTime) -> Result<()> {
        self.set(&format!(":SG{}{:03}#", if st.utc_offset_minutes < 0 { '-' } else { '+' }, st.utc_offset_minutes.abs()))?;
        self.set(":SDS0#")?;
        let ms = st.unix_secs * 1000 - J2000_UNIX_MS;
        self.set(&format!(":SUT{ms:013}#"))?;
        self.set(&format!(":SLO{}#", signed8(st.longitude_east_deg)))?;
        self.set(&format!(":SLA{}#", signed8(st.latitude_deg)))?;
        self.set(if st.latitude_deg >= 0.0 { ":SHE1#" } else { ":SHE0#" })
    }

    /// `:SRA<9 digits>#`, `:Sd<sign 8 digits>#` (case matters in this protocol), then `:MS1#` (the normal position; `:MS2#`, counterweight up, if the first is out of reach).
    /// The mount tracks by itself when the slew ends.
    fn goto_equatorial(&mut self, ra_hours: f64, dec_deg: f64) -> Result<()> {
        let ra = ((ra_hours.rem_euclid(24.0) / 24.0) * FULL_CIRCLE).round() as u64 % FULL_CIRCLE as u64;
        self.set(&format!(":SRA{ra:09}#"))?;
        self.set(&format!(":Sd{}#", signed8(dec_deg.clamp(-90.0, 90.0))))?;
        if send(&mut self.port, ":MS1#", Reply::One).is_err() {
            send(&mut self.port, ":MS2#", Reply::One).map_err(|_| Error::Mount("the mount refused the GoTo: the target is below its altitude limit or outside its mechanical limits".into()))?;
        }
        self.tracking = true;
        Ok(())
    }

    fn go_home(&mut self) -> Result<()> {
        self.set(":MH#")?;
        self.tracking = false;
        Ok(())
    }

    fn set_tracking(&mut self, enabled: bool) -> Result<()> {
        if let Err(e) = self.set(if enabled { ":ST1#" } else { ":ST0#" }) {
            return Err(match (&e, enabled) {
                (Error::Mount(m), true) if m.contains("got \"0\"") => Error::Mount(format!("the mount would not start tracking: {}", self.why_not_tracking().unwrap_or_else(|| "it gave no reason".into()))),
                _ => e,
            });
        }
        self.tracking = enabled;
        Ok(())
    }

    fn set_tracking_rate(&mut self, rate: TrackingRate) -> Result<()> {
        self.set(match rate {
            TrackingRate::Sidereal => ":RT0#",
            TrackingRate::Lunar => ":RT1#",
            TrackingRate::Solar => ":RT2#",
        })
    }

    /// Guiding by timed pulses: `:ZS`/`:ZQ` RA+/RA-, `:ZE`/`:ZC` Dec+/Dec-, each for up to 99,999 ms at the guiding rate (`:RGrrdd#`,
    /// RA 0.01 to 0.90, Dec 0.10 to 0.99 of sidereal). A pulse of 0 ms ends a pulse. Which way is which on the sky is learned by the calibration.
    fn guide(&mut self, a1: RadPerSec, a2: RadPerSec) -> Result<()> {
        let moving = !(a1.is_zero() && a2.is_zero());
        if moving && !self.tracking {
            return Err(Error::Mount("cannot guide when tracking is disabled".into()));
        }
        if moving {
            let ra = ((a1.0.abs() / SIDEREAL_RATE.0).clamp(0.01, 0.90) * 100.0).round() as u8;
            let dec = ((a2.0.abs() / SIDEREAL_RATE.0).clamp(0.10, 0.99) * 100.0).round() as u8;
            let (ra, dec) = (if a1.is_zero() { self.guide_rates.map_or(50, |g| g.0) } else { ra }, if a2.is_zero() { self.guide_rates.map_or(50, |g| g.1) } else { dec });
            if self.guide_rates != Some((ra, dec)) {
                self.set(&format!(":RG{ra:02}{dec:02}#"))?;
                self.guide_rates = Some((ra, dec));
            }
        }
        for (v, plus, minus) in [(a1.0, "ZS", "ZQ"), (a2.0, "ZE", "ZC")] {
            if v > 0.0 {
                self.cmd(&format!(":{plus}{GUIDE_PULSE_MS:05}#"))?;
            } else if v < 0.0 {
                self.cmd(&format!(":{minus}{GUIDE_PULSE_MS:05}#"))?;
            } else {
                self.cmd(&format!(":{plus}00000#"))?;
                self.cmd(&format!(":{minus}00000#"))?;
            }
        }
        Ok(())
    }

    /// `:SRn#` picks the moving rate, `:mw#`/`:me#` move RA+/RA-, `:ms#`/`:mn#` Dec+/Dec-; `:qR#` and `:qD#` stop one axis.
    fn slew(&mut self, axis: Axis, speed: SlewSpeed) -> Result<()> {
        if speed.is_zero() {
            return self.set(if axis == Axis::Primary { ":qR#" } else { ":qD#" });
        }
        let n = move_rate(&speed).ok_or_else(|| Error::Unsupported("slewing speed".into()))?;
        self.set(&format!(":SR{n}#"))?;
        self.cmd(match (axis, speed.positive()) {
            (Axis::Primary, true) => ":mw#",
            (Axis::Primary, false) => ":me#",
            (Axis::Secondary, true) => ":ms#",
            (Axis::Secondary, false) => ":mn#",
        })
    }

    fn slewing_speed_supported(&self, speed: RadPerSec) -> bool {
        move_rate(&SlewSpeed::Specific(speed)).is_some()
    }

    /// `:Q#` stops every slew; tracking is not touched.
    fn stop(&mut self) -> Result<()> {
        self.set(":Q#")?;
        self.set(":qR#")?;
        self.set(":qD#")
    }

    fn query(&mut self, what: &str) -> Result<String> {
        // read-only getters only
        const ALLOWED: [&str; 12] = ["GLS", "GUT", "GEP", "GAC", "GTR", "GPC", "GSR", "GAL", "AG", "GMT", "FW1", "FW2"];
        if what == "MountInfo" {
            return Ok(String::from_utf8_lossy(&send(&mut self.port, ":MountInfo#", Reply::Fixed(4))?).to_string());
        }
        if !ALLOWED.contains(&what) {
            return Err(Error::Unsupported(format!("query {what}: allowed are MountInfo, {}", ALLOWED.join(", "))));
        }
        let r = if what.starts_with("FW") { send(&mut self.port, &format!(":{what}#"), Reply::Hash) } else { send(&mut self.port, &format!(":{what}#"), Reply::Hash) }?;
        Ok(String::from_utf8_lossy(&r).to_string())
    }

    fn why_not_tracking(&mut self) -> Option<String> {
        let s = self.status_digit().ok()?;
        let alt_limit = send(&mut self.port, ":GAL#", Reply::Hash).ok().map(|r| String::from_utf8_lossy(&r).to_string());
        Some(match s {
            1 | 3 | 5 => "it is tracking".into(),
            2 | 4 => "it is slewing or flipping".into(),
            6 => "it is parked (unpark it from its hand controller or its own app)".into(),
            7 => "it is stopped at its zero (home) position".into(),
            _ => format!("it is stopped; a mount stops tracking when it passes its altitude limit (now {}) or meridian limit", alt_limit.as_deref().unwrap_or("unknown")),
        })
    }
}

/// `s` and 8 digits in 0.01 arc-seconds: `+00000000` form.
fn signed8(deg: f64) -> String {
    let v = (deg.abs() * CENTI_ARCSEC_PER_DEG).round() as i64;
    format!("{}{v:08}", if deg < 0.0 { '-' } else { '+' })
}

/// `:GEP#` reply: sign, 8 digits declination, 9 digits right ascension, 2 digits (pier side, pointing state).
fn parse_gep(reply: &str) -> Option<(f64, f64)> {
    let r = reply.trim();
    if r.len() < 18 {
        return None;
    }
    let sign = match r.as_bytes()[0] { b'-' => -1.0, b'+' => 1.0, _ => return None };
    let dec: f64 = r.get(1..9)?.parse().ok()?;
    let ra: f64 = r.get(9..18)?.parse().ok()?;
    Some((ra / FULL_CIRCLE * 24.0, sign * dec / CENTI_ARCSEC_PER_DEG))
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
    let want = match reply {
        Reply::None => return Ok(vec![]),
        Reply::One => Some(1),
        Reply::Fixed(n) => Some(n),
        Reply::Hash => None,
    };
    let mut buf = Vec::new();
    loop {
        let mut b = [0u8; 1];
        dev.read_exact(&mut b).map_err(|e| Error::Mount(format!("cmd {cmd}: no reply ({e})")))?;
        if matches!(reply, Reply::Hash) && b[0] == b'#' {
            break;
        }
        buf.push(b[0]);
        if buf.len() > 256 {
            return Err(Error::Mount("reply too long".into()));
        }
        if Some(buf.len()) == want {
            break;
        }
    }
    if matches!(reply, Reply::One) && buf != b"1" {
        return Err(Error::Mount(format!("cmd {cmd}: expected \"1\", got {:?}", String::from_utf8_lossy(&buf))));
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

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

    /// A `:GLS#` reply with the given system status digit.
    fn gls(status: u8) -> String {
        // longitude -96.5 deg, latitude 29.4 + 90, gps 2, status, rate 2 (solar), speed 6, time source 1, northern hemisphere
        format!("-34740000{:08}2{status}2611#", ((29.4 + 90.0) * 360_000.0) as u64)
    }

    fn mount(extra: &str) -> Ioptron<Mock> {
        Ioptron::new(Mock { written: String::new(), replies: format!("0070{}{extra}", gls(0)).bytes().collect() }, "mock").unwrap()
    }

    #[test]
    fn identifies_the_model_first_and_reads_the_state_without_changing_it() {
        let m = mount("");
        assert_eq!(m.info(), "iOptron CEM70 on mock");
        assert_eq!(m.port.written, ":MountInfo#:GLS#");
        assert!(!m.tracking);
    }

    #[test]
    fn a_port_that_does_not_answer_is_reported_with_the_likely_cause() {
        let e = Ioptron::new(Mock { written: String::new(), replies: VecDeque::new() }, "mock").err().expect("must fail");
        assert!(e.to_string().contains("115200"), "{e}");
        let e = Ioptron::new(Mock { written: String::new(), replies: "AM5#".bytes().collect() }, "mock").err().expect("must fail");
        assert!(e.to_string().contains("four digits"), "{e}");
    }

    #[test]
    fn state_follows_the_system_status_digit() {
        for (digit, tracking, slewing, home, park) in [(1, true, false, false, ParkState::Unparked), (2, false, true, false, ParkState::Unparked), (6, false, false, false, ParkState::Parked), (7, false, false, true, ParkState::Unparked), (3, true, false, false, ParkState::Unparked)] {
            let mut m = mount(&gls(digit));
            let s = m.state().unwrap();
            assert_eq!((s.tracking, s.slewing, s.at_home, s.park), (Some(tracking), Some(slewing), Some(home), Some(park)), "status {digit}");
        }
    }

    #[test]
    fn pointing_is_parsed_from_gep() {
        // dec +20 deg = 7,200,000; RA 6 h = 32,400,000 (a quarter of 129,600,000)
        let (ra, dec) = parse_gep("+07200000032400000" .to_string().as_str()).unwrap();
        assert!((ra - 6.0).abs() < 1e-9 && (dec - 20.0).abs() < 1e-9, "{ra} {dec}");
        let (_, dec) = parse_gep("-00360000000000000010").unwrap();
        assert!((dec + 1.0).abs() < 1e-9);
        assert!(parse_gep("x").is_none());
    }

    #[test]
    fn site_and_time_use_the_protocols_units() {
        let mut m = mount("111111");
        m.port.written.clear();
        // 2026-11-26 22:00:00 UTC, site 29.5 N 96 W, UTC-6
        m.set_site_time(&SiteTime { latitude_deg: 29.5, longitude_east_deg: -96.0, unix_secs: 1_795_816_800, utc_offset_minutes: -360 }).unwrap();
        let w = &m.port.written;
        assert!(w.contains(":SG-360#") && w.contains(":SDS0#") && w.contains(":SLO-34560000#") && w.contains(":SLA+10620000#") && w.contains(":SHE1#"), "{w}");
        let ms = 1_795_816_800i64 * 1000 - 946_728_000_000;
        assert!(w.contains(&format!(":SUT{ms:013}#")), "{w}");
    }

    #[test]
    fn goto_sets_the_target_then_slews_and_falls_back_to_the_counterweight_up_position() {
        let mut m = mount("11");
        m.port.replies.extend(b"0");
        m.port.replies.extend(b"1");
        m.port.written.clear();
        m.goto_equatorial(12.0, -5.5).unwrap();
        assert_eq!(m.port.written, ":SRA064800000#:Sd-01980000#:MS1#:MS2#");
        assert!(m.tracking, "the mount tracks by itself after a slew");
        // refused both ways: says why
        let mut m = mount("1100");
        let e = m.goto_equatorial(1.0, 1.0).unwrap_err();
        assert!(e.to_string().contains("altitude limit"), "{e}");
    }

    #[test]
    fn tracking_and_rates() {
        let mut m = mount("111");
        m.port.written.clear();
        m.set_tracking_rate(TrackingRate::Solar).unwrap();
        m.set_tracking(true).unwrap();
        m.set_tracking_rate(TrackingRate::Lunar).unwrap();
        assert_eq!(m.port.written, ":RT2#:ST1#:RT1#");
    }

    #[test]
    fn guide_sets_the_rate_once_and_sends_timed_pulses() {
        let mut m = mount("1");
        m.set_tracking(true).unwrap();
        m.port.replies.extend(b"1");
        m.port.written.clear();
        m.guide(RadPerSec(0.5 * SIDEREAL_RATE.0), RadPerSec(-0.5 * SIDEREAL_RATE.0)).unwrap();
        assert_eq!(m.port.written, ":RG5050#:ZS02500#:ZC02500#");
        m.port.written.clear();
        m.guide(RadPerSec(0.5 * SIDEREAL_RATE.0), RadPerSec(0.0)).unwrap();
        assert_eq!(m.port.written, ":ZS02500#:ZE00000#:ZC00000#", "the rate is not sent again, and the idle axis is told to stop");
        let mut m = mount("");
        assert!(m.guide(RadPerSec(SIDEREAL_RATE.0 * 0.5), RadPerSec(0.0)).unwrap_err().to_string().contains("tracking is disabled"));
    }

    #[test]
    fn manual_slews_pick_a_rate_and_a_direction_and_stop_per_axis() {
        let mut m = mount("111");
        m.port.written.clear();
        m.slew(Axis::Primary, SlewSpeed::Specific(RadPerSec(64.0 * SIDEREAL_RATE.0))).unwrap();
        m.slew(Axis::Secondary, SlewSpeed::Specific(RadPerSec(-8.0 * SIDEREAL_RATE.0))).unwrap();
        assert_eq!(m.port.written, ":SR5#:mw#:SR3#:mn#");
        m.port.replies.extend(b"11");
        m.port.written.clear();
        m.slew(Axis::Primary, SlewSpeed::zero()).unwrap();
        m.slew(Axis::Secondary, SlewSpeed::zero()).unwrap();
        assert_eq!(m.port.written, ":qR#:qD#");
        assert!(m.slewing_speed_supported(RadPerSec(16.0 * SIDEREAL_RATE.0)));
        assert!(!m.slewing_speed_supported(RadPerSec(20.0 * SIDEREAL_RATE.0)));
    }

    #[test]
    fn only_listed_getters_can_be_queried() {
        let mut m = mount("");
        assert!(m.query("MS1").is_err());
        assert!(m.query(":Q").is_err());
    }
}
