//! Does the mount hold the same site and time as the Pi? (Issue #104.) Pure comparison; the engine reads the mount and publishes this.

use rustflare_core::MountSiteTime;
use serde::Serialize;
use std::time::Duration;

/// The mount's clock may differ from the Pi's by this many seconds before it is called different. A Go to Sun is computed from the
/// time the mount holds; a few seconds of Sun motion is a few arc-seconds, far inside the frame.
pub const TIME_TOLERANCE_S: f64 = 5.0;
/// The mount's site may differ from the saved site by this many degrees (about 5.5 km) before it is called different. A place error
/// shifts the computed pointing by about as much, which is still inside a small telescope's field only for errors this small.
pub const PLACE_TOLERANCE_DEG: f64 = 0.05;
/// How often the engine asks the mount for its site and clock. Each iOptron command can take 300 ms, so not with every 2 s poll.
pub const READ_INTERVAL: Duration = Duration::from_secs(30);

/// A site and a clock, either the mount's or the Pi's. Fields are `None` when unknown (not reported, or no site saved).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct ClockPlace {
    pub latitude_deg: Option<f64>,
    pub longitude_east_deg: Option<f64>,
    pub unix_secs: Option<i64>,
    pub utc_offset_minutes: Option<i32>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SiteTimeStatus {
    /// When the mount was last asked (Unix seconds, the Pi's clock).
    pub read_at_unix: i64,
    pub mount: ClockPlace,
    pub pi: ClockPlace,
    /// Mount clock minus the Pi's, seconds (positive: the mount is ahead). `None` when the mount reported no time.
    pub time_diff_s: Option<f64>,
    /// Angular distance between the two sites, degrees. `None` when either is unknown.
    pub place_diff_deg: Option<f64>,
    /// `None`: not comparable.
    pub time_ok: Option<bool>,
    pub place_ok: Option<bool>,
    /// False when something that could be compared differs by more than the tolerance.
    pub ok: bool,
    pub why: String,
}

/// Angular distance in degrees between two points on the sphere (haversine, so a longitude step across 180 is small).
pub fn place_distance_deg(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dl = (lon2 - lon1).to_radians();
    let h = ((p2 - p1) / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    (2.0 * h.sqrt().min(1.0).asin()).to_degrees()
}

/// "3 min 12 s", "45 s", "2 h 5 min": a time difference in words.
fn words(secs: f64) -> String {
    let s = secs.abs().round() as i64;
    match s {
        0..=59 => format!("{s} s"),
        60..=3599 => if s % 60 == 0 { format!("{} min", s / 60) } else { format!("{} min {} s", s / 60, s % 60) },
        _ => if s % 3600 / 60 == 0 { format!("{} h", s / 3600) } else { format!("{} h {} min", s / 3600, s % 3600 / 60) },
    }
}

/// Compares what the mount holds with the Pi's saved site and clock. `pi_unix` is the Pi's clock when the mount was read.
pub fn compare(mount: MountSiteTime, pi_site: Option<(f64, f64)>, pi_unix: f64, pi_offset_minutes: i32, read_at_unix: i64) -> SiteTimeStatus {
    let time_diff_s = mount.unix_secs.map(|t| t as f64 - pi_unix);
    let place_diff_deg = match (mount.latitude_deg, mount.longitude_east_deg, pi_site) {
        (Some(la), Some(lo), Some((pla, plo))) => Some(place_distance_deg(la, lo, pla, plo)),
        _ => None,
    };
    let time_ok = time_diff_s.map(|d| d.abs() <= TIME_TOLERANCE_S);
    let place_ok = place_diff_deg.map(|d| d <= PLACE_TOLERANCE_DEG);
    let ok = time_ok != Some(false) && place_ok != Some(false);
    let mut parts: Vec<String> = Vec::new();
    if let (Some(d), Some(false)) = (time_diff_s, time_ok) {
        parts.push(format!("the mount's clock is {} {} the Pi's", words(d), if d > 0.0 { "ahead of" } else { "behind" }));
    }
    if let (Some(d), Some(false)) = (place_diff_deg, place_ok) {
        parts.push(format!("the mount's place is {d:.2} degrees from the saved site"));
    }
    let why = if !parts.is_empty() {
        let mut t = parts.join(", and ");
        t.replace_range(0..1, &t[0..1].to_uppercase());
        format!("{t}. Send the site and time to the mount.")
    } else {
        let mut notes = Vec::new();
        if mount.unix_secs.is_none() { notes.push("the mount does not report its clock"); }
        if mount.latitude_deg.is_none() || mount.longitude_east_deg.is_none() { notes.push("the mount does not report its place"); }
        else if pi_site.is_none() { notes.push("no site is saved to compare its place with"); }
        if notes.is_empty() { "The mount's clock and place match the Pi's.".into() } else { format!("Nothing differs, but {}.", notes.join(" and ")) }
    };
    SiteTimeStatus {
        read_at_unix,
        mount: ClockPlace { latitude_deg: mount.latitude_deg, longitude_east_deg: mount.longitude_east_deg, unix_secs: mount.unix_secs, utc_offset_minutes: mount.utc_offset_minutes },
        pi: ClockPlace { latitude_deg: pi_site.map(|s| s.0), longitude_east_deg: pi_site.map(|s| s.1), unix_secs: Some(pi_unix.round() as i64), utc_offset_minutes: Some(pi_offset_minutes) },
        time_diff_s,
        place_diff_deg,
        time_ok,
        place_ok,
        ok,
        why,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mount(lat: f64, lon: f64, unix: i64) -> MountSiteTime {
        MountSiteTime { latitude_deg: Some(lat), longitude_east_deg: Some(lon), unix_secs: Some(unix), utc_offset_minutes: Some(-360) }
    }

    #[test]
    fn a_mount_inside_both_tolerances_matches() {
        let c = compare(mount(40.0, -105.0, 1_000_004), Some((40.0, -105.0)), 1_000_000.0, -360, 1_000_000);
        assert!(c.ok && c.time_ok == Some(true) && c.place_ok == Some(true), "{c:?}");
        assert_eq!(c.time_diff_s, Some(4.0));
        assert!(c.why.contains("match"), "{}", c.why);
    }

    #[test]
    fn the_time_tolerance_is_five_seconds_either_way() {
        for (d, ok) in [(5, true), (-5, true), (6, false), (-6, false), (180, false)] {
            let c = compare(mount(40.0, -105.0, 1_000_000 + d), Some((40.0, -105.0)), 1_000_000.0, 0, 0);
            assert_eq!(c.ok, ok, "{d} s");
        }
        let c = compare(mount(40.0, -105.0, 1_000_192), Some((40.0, -105.0)), 1_000_000.0, 0, 0);
        assert!(c.why.contains("3 min 12 s ahead"), "{}", c.why);
        let c = compare(mount(40.0, -105.0, 999_000), Some((40.0, -105.0)), 1_000_000.0, 0, 0);
        assert!(c.why.contains("16 min 40 s behind"), "{}", c.why);
    }

    #[test]
    fn the_place_tolerance_is_a_twentieth_of_a_degree_as_an_angle() {
        let ok = compare(mount(40.04, -105.0, 0), Some((40.0, -105.0)), 0.0, 0, 0);
        assert!(ok.ok, "{ok:?}");
        let off = compare(mount(40.06, -105.0, 0), Some((40.0, -105.0)), 0.0, 0, 0);
        assert!(!off.ok && off.place_ok == Some(false) && off.why.contains("0.06 degrees"), "{off:?}");
        // a longitude step shrinks with latitude: 0.06 degrees of longitude at 60 N is 0.03 degrees of arc
        assert!(compare(mount(60.0, 10.06, 0), Some((60.0, 10.0)), 0.0, 0, 0).ok);
    }

    #[test]
    fn longitude_wraps_at_180_degrees() {
        assert!(place_distance_deg(0.0, 179.99, 0.0, -179.99) < 0.021);
        assert!(compare(mount(0.0, 179.99, 0), Some((0.0, -179.99)), 0.0, 0, 0).ok);
        // a mount holding a site far from the saved one
        let c = compare(mount(10.0, -80.0, 0), Some((40.0, -105.0)), 0.0, 0, 0);
        assert!(!c.ok && c.place_diff_deg.unwrap() > 30.0);
        assert!((place_distance_deg(0.0, 0.0, 0.0, 180.0) - 180.0).abs() < 1e-9);
    }

    #[test]
    fn what_cannot_be_compared_is_not_called_different() {
        // a ZWO mount: clock only
        let zwo = MountSiteTime { latitude_deg: None, longitude_east_deg: None, unix_secs: Some(50), utc_offset_minutes: Some(-360) };
        let c = compare(zwo, Some((40.0, -105.0)), 50.0, -360, 0);
        assert!(c.ok && c.place_ok.is_none() && c.place_diff_deg.is_none() && c.why.contains("does not report its place"), "{c:?}");
        assert!(!compare(zwo, None, 500.0, -360, 0).ok, "the clock still counts without a saved site");
        // no saved site: the place cannot be judged
        let c = compare(mount(10.0, 10.0, 0), None, 0.0, 0, 0);
        assert!(c.ok && c.why.contains("no site is saved"), "{}", c.why);
    }
}
