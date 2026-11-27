//! Low-precision positions of the Moon and the bright planets for "Go to" targets.
//!
//! Planets: the JPL "Keplerian elements for approximate positions" (valid 1800 to 2050, a few arcminutes), precessed
//! to the equinox of date like the Sun function. Moon: the Astronomical Almanac low-precision series (about 0.3 degree)
//! with the topocentric parallax, which for the Moon is up to a degree. This is enough to put the target in the
//! field of a camera on a slow GoTo; centre it by hand from there. Nothing here is meant for anything finer.

use crate::solar::{alt_az, local_sidereal_hours};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Body {
    Moon,
    Mars,
    Jupiter,
    Saturn,
}

impl Body {
    pub const ALL: [Body; 4] = [Body::Moon, Body::Mars, Body::Jupiter, Body::Saturn];

    pub fn id(self) -> &'static str {
        match self { Body::Moon => "moon", Body::Mars => "mars", Body::Jupiter => "jupiter", Body::Saturn => "saturn" }
    }

    pub fn name(self) -> &'static str {
        match self { Body::Moon => "Moon", Body::Mars => "Mars", Body::Jupiter => "Jupiter", Body::Saturn => "Saturn" }
    }

    pub fn from_id(id: &str) -> Option<Body> {
        Body::ALL.into_iter().find(|b| b.id() == id)
    }
}

/// Orbital elements at J2000 and their change per Julian century: a (AU), e, I, L, varpi, Omega (degrees).
struct Elements { a: [f64; 2], e: [f64; 2], i: [f64; 2], l: [f64; 2], w: [f64; 2], o: [f64; 2] }

const EARTH: Elements = Elements { a: [1.00000261, 0.00000562], e: [0.01671123, -0.00004392], i: [-0.00001531, -0.01294668], l: [100.46457166, 35999.37244981], w: [102.93768193, 0.32327364], o: [0.0, 0.0] };
const MARS: Elements = Elements { a: [1.52371034, 0.00001847], e: [0.09339410, 0.00007882], i: [1.84969142, -0.00813131], l: [-4.55343205, 19140.30268499], w: [-23.94362959, 0.44441088], o: [49.55953891, -0.29257343] };
const JUPITER: Elements = Elements { a: [5.20288700, -0.00011607], e: [0.04838624, -0.00013253], i: [1.30439695, -0.00183714], l: [34.39644051, 3034.74612775], w: [14.72847983, 0.21252668], o: [100.47390909, 0.20469106] };
const SATURN: Elements = Elements { a: [9.53667594, -0.00125060], e: [0.05386179, -0.00050991], i: [2.48599187, 0.00193609], l: [49.95424423, 1222.49362201], w: [92.59887831, -0.41897216], o: [113.66242448, -0.28867794] };

/// Heliocentric ecliptic (J2000) rectangular coordinates in AU.
fn heliocentric(el: &Elements, t: f64) -> [f64; 3] {
    let at = |p: [f64; 2]| p[0] + p[1] * t;
    let (a, e, inc, l, w, o) = (at(el.a), at(el.e), at(el.i).to_radians(), at(el.l), at(el.w), at(el.o).to_radians());
    let m = (l - w).rem_euclid(360.0).to_radians();
    let mut ea = m + e * m.sin(); // Kepler's equation by iteration (e < 0.1 here)
    for _ in 0..8 {
        ea -= (ea - e * ea.sin() - m) / (1.0 - e * ea.cos());
    }
    let (xp, yp) = (a * (ea.cos() - e), a * (1.0 - e * e).sqrt() * ea.sin());
    let om = (w - at(el.o)).to_radians(); // argument of perihelion
    let (co, so, ci, si, cw, sw) = (o.cos(), o.sin(), inc.cos(), inc.sin(), om.cos(), om.sin());
    [(cw * co - sw * so * ci) * xp + (-sw * co - cw * so * ci) * yp, (cw * so + sw * co * ci) * xp + (-sw * so + cw * co * ci) * yp, sw * si * xp + cw * si * yp]
}

/// Ecliptic longitude and latitude (degrees) to equatorial (RA hours, Dec degrees) for an obliquity in degrees.
fn ecliptic_to_equatorial(lon_deg: f64, lat_deg: f64, eps_deg: f64) -> (f64, f64) {
    let (lon, lat, eps) = (lon_deg.to_radians(), lat_deg.to_radians(), eps_deg.to_radians());
    let ra = (lon.sin() * eps.cos() - lat.tan() * eps.sin()).atan2(lon.cos()).to_degrees().rem_euclid(360.0) / 15.0;
    let dec = (lat.sin() * eps.cos() + lat.cos() * eps.sin() * lon.sin()).asin().to_degrees();
    (ra, dec)
}

/// Geocentric ecliptic (J2000) rectangular coordinates of a planet in AU, and the time in Julian centuries since J2000.
fn planet_vector(body: Body, unix_secs: f64) -> ([f64; 3], f64) {
    let jd = unix_secs / 86400.0 + 2440587.5;
    let t = (jd - 2451545.0) / 36525.0;
    let el = match body { Body::Mars => &MARS, Body::Jupiter => &JUPITER, _ => &SATURN };
    let (p, e) = (heliocentric(el, t), heliocentric(&EARTH, t));
    ([p[0] - e[0], p[1] - e[1], p[2] - e[2]], t)
}

fn planet_equatorial(body: Body, unix_secs: f64) -> (f64, f64) {
    let ([x, y, z], t) = planet_vector(body, unix_secs);
    let lon = y.atan2(x).to_degrees() + 1.3969713 * t; // precess J2000 to the equinox of date
    let lat = z.atan2((x * x + y * y).sqrt()).to_degrees();
    ecliptic_to_equatorial(lon.rem_euclid(360.0), lat, 23.439291 - 0.0130042 * t)
}

/// Equatorial radius of the Moon over the Earth's (k in the almanac's sin(radius) = k sin(parallax)).
const MOON_K: f64 = 0.2725076;
/// Equatorial radii of the planets' globes (rings excluded), km, and the astronomical unit, km.
const PLANET_RADIUS_KM: [(Body, f64); 3] = [(Body::Mars, 3396.2), (Body::Jupiter, 71492.0), (Body::Saturn, 60268.0)];
const AU_KM: f64 = 149_597_870.7;

/// Apparent angular radius of a body in arcseconds at `unix_secs`: the Moon from its distance (the series' parallax, 14.7 to 16.7
/// arcminutes over the month), a planet's globe from its distance in AU (Jupiter 15 to 25, Mars 2 to 12, Saturn 7 to 10 arcseconds).
pub fn angular_radius_arcsec(body: Body, unix_secs: f64) -> f64 {
    match body {
        Body::Moon => {
            let t = (unix_secs / 86400.0 + 2440587.5 - 2451545.0) / 36525.0;
            let c = |a: f64, b: f64| (a + b * t).to_radians().cos();
            let par = (0.9508 + 0.0518 * c(135.0, 477198.87) + 0.0095 * c(259.3, -413335.36) + 0.0078 * c(235.7, 890534.22) + 0.0028 * c(269.9, 954397.74)).to_radians();
            (MOON_K * par.sin()).asin().to_degrees() * 3600.0
        }
        _ => {
            let ([x, y, z], _) = planet_vector(body, unix_secs);
            let dist_km = (x * x + y * y + z * z).sqrt() * AU_KM;
            let r = PLANET_RADIUS_KM.iter().find(|(b, _)| *b == body).map_or(0.0, |p| p.1);
            (r / dist_km).atan().to_degrees() * 3600.0
        }
    }
}

/// How fast the hour angle of a body grows, in hours of hour angle per hour of time (the Sun: about 1.0, a star 1.0027, the Moon
/// 0.966 because it moves east through the stars). Measured over ten minutes from the positions themselves.
pub fn hour_angle_rate(body: Body, unix_secs: f64, latitude_deg: f64, longitude_east_deg: f64) -> f64 {
    const DT: f64 = 600.0;
    let ha = |t: f64| (local_sidereal_hours(t, longitude_east_deg) - equatorial(body, t, latitude_deg, longitude_east_deg).0).rem_euclid(24.0);
    let mut d = ha(unix_secs + DT) - ha(unix_secs);
    if d < -12.0 { d += 24.0 } else if d > 12.0 { d -= 24.0 }
    d / (DT / 3600.0)
}

/// The Moon as seen from the observer (topocentric), at `unix_secs`.
fn moon_equatorial(unix_secs: f64, latitude_deg: f64, longitude_east_deg: f64) -> (f64, f64) {
    let t = (unix_secs / 86400.0 + 2440587.5 - 2451545.0) / 36525.0;
    let s = |a: f64, b: f64| (a + b * t).to_radians().sin();
    let c = |a: f64, b: f64| (a + b * t).to_radians().cos();
    let lon = 218.32 + 481267.881 * t + 6.29 * s(135.0, 477198.87) - 1.27 * s(259.3, -413335.36) + 0.66 * s(235.7, 890534.22) + 0.21 * s(269.9, 954397.74) - 0.19 * s(357.5, 35999.05) - 0.11 * s(186.5, 966404.03);
    let lat = 5.13 * s(93.3, 483202.02) + 0.28 * s(228.2, 960400.89) - 0.28 * s(318.3, 6003.15) - 0.17 * s(217.6, -407332.21);
    let par = (0.9508 + 0.0518 * c(135.0, 477198.87) + 0.0095 * c(259.3, -413335.36) + 0.0078 * c(235.7, 890534.22) + 0.0028 * c(269.9, 954397.74)).to_radians();
    let (lo, la) = (lon.to_radians(), lat.to_radians());
    let (l, m, n) = (la.cos() * lo.cos(), 0.9175 * la.cos() * lo.sin() - 0.3978 * la.sin(), 0.3978 * la.cos() * lo.sin() + 0.9175 * la.sin());
    // geocentric vector in Earth radii, minus the observer's position
    let dist = 1.0 / par.sin();
    let lst = (local_sidereal_hours(unix_secs, longitude_east_deg) * 15.0).to_radians();
    let phi = latitude_deg.to_radians();
    let (ox, oy, oz) = (phi.cos() * lst.cos(), phi.cos() * lst.sin(), phi.sin());
    let (x, y, z) = (l * dist - ox, m * dist - oy, n * dist - oz);
    ((y.atan2(x).to_degrees().rem_euclid(360.0)) / 15.0, z.atan2((x * x + y * y).sqrt()).to_degrees())
}

/// Apparent position of a body (RA hours, Dec degrees, equinox of date) for an observer.
pub fn equatorial(body: Body, unix_secs: f64, latitude_deg: f64, longitude_east_deg: f64) -> (f64, f64) {
    match body {
        Body::Moon => moon_equatorial(unix_secs, latitude_deg, longitude_east_deg),
        _ => planet_equatorial(body, unix_secs),
    }
}

/// Altitude and azimuth (degrees) of a body for an observer.
pub fn altitude_azimuth(body: Body, unix_secs: f64, latitude_deg: f64, longitude_east_deg: f64) -> (f64, f64, f64, f64) {
    let (ra, dec) = equatorial(body, unix_secs, latitude_deg, longitude_east_deg);
    let (alt, az) = alt_az(ra, dec, unix_secs, latitude_deg, longitude_east_deg);
    (ra, dec, alt, az)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solar::sun_equatorial;

    fn unix(y: i64, m: i64, d: i64, h: i64, mi: i64) -> f64 {
        let (y2, m2) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
        let era = y2.div_euclid(400);
        let yoe = y2 - era * 400;
        let doy = (153 * m2 + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        ((era * 146097 + doe - 719468) * 86400 + h * 3600 + mi * 60) as f64
    }

    /// Angular separation in degrees between two equatorial positions.
    fn sep(a: (f64, f64), b: (f64, f64)) -> f64 {
        let (r1, d1, r2, d2) = ((a.0 * 15.0).to_radians(), a.1.to_radians(), (b.0 * 15.0).to_radians(), b.1.to_radians());
        (d1.sin() * d2.sin() + d1.cos() * d2.cos() * (r1 - r2).cos()).clamp(-1.0, 1.0).acos().to_degrees()
    }

    /// Ecliptic longitude (degrees) of an equatorial position, for the obliquity of 2025 to 2026.
    fn ecliptic_longitude(p: (f64, f64)) -> f64 {
        let (ra, dec, eps) = ((p.0 * 15.0).to_radians(), p.1.to_radians(), 23.436f64.to_radians());
        (ra.sin() * eps.cos() + dec.tan() * eps.sin()).atan2(ra.cos()).to_degrees().rem_euclid(360.0)
    }

    /// Opposition means the ecliptic longitude is 180 degrees from the Sun's (the planet's latitude makes the angular
    /// separation a little smaller, so compare longitudes).
    #[test]
    fn angular_sizes_are_the_known_ones() {
        let t = unix(2026, 11, 24, 15, 0);
        let moon = angular_radius_arcsec(Body::Moon, t);
        assert!((880.0..1010.0).contains(&moon), "Moon radius {moon} arcsec (14.7 to 16.7 arcmin)");
        let j = angular_radius_arcsec(Body::Jupiter, t);
        assert!((14.0..26.0).contains(&j), "Jupiter {j}");
        let m = angular_radius_arcsec(Body::Mars, t);
        assert!((1.5..12.5).contains(&m), "Mars {m}");
        let s = angular_radius_arcsec(Body::Saturn, t);
        assert!((6.5..10.5).contains(&s), "Saturn {s}");
    }

    #[test]
    fn the_moon_hour_angle_grows_slower_than_a_stars() {
        let t = unix(2026, 11, 24, 15, 0);
        let moon = hour_angle_rate(Body::Moon, t, 40.0, -90.0);
        let jup = hour_angle_rate(Body::Jupiter, t, 40.0, -90.0);
        assert!((0.92..0.99).contains(&moon), "Moon {moon}"); // 0.966 geocentric; the parallax of an observer near the Moon moves it by a few per cent
        assert!((1.0020..1.0035).contains(&jup), "Jupiter {jup}");
    }

    #[test]
    fn planets_are_opposite_the_sun_at_opposition() {
        for (body, when) in [(Body::Jupiter, unix(2026, 1, 10, 12, 0)), (Body::Saturn, unix(2025, 9, 21, 12, 0)), (Body::Mars, unix(2025, 1, 16, 12, 0))] {
            let d = (ecliptic_longitude(equatorial(body, when, 0.0, 0.0)) - ecliptic_longitude(sun_equatorial(when))).rem_euclid(360.0);
            assert!((d - 180.0).abs() < 1.5, "{}: {d:.2} degrees from the Sun in longitude at its opposition", body.name());
        }
    }

    /// The total eclipse of 2026-08-12 (greatest 17:46 UTC): the Moon is within about a degree of the Sun, geocentric
    /// (observer at the Earth's centre of view: the pole of the sub-lunar point is not needed at this precision).
    #[test]
    fn moon_meets_the_sun_at_the_2026_eclipse() {
        let when = unix(2026, 8, 12, 17, 46);
        // an observer in Spain, under the path of totality, sees the Moon over the Sun
        let e = sep(equatorial(Body::Moon, when, 42.0, -3.0), sun_equatorial(when));
        assert!(e < 0.8, "Moon is {e:.2} degrees from the Sun during the eclipse");
    }

    /// Full Moon on 2026-11-24 about 14:53 UTC: 180 degrees from the Sun in longitude to within a couple of degrees (the Moon moves 0.5 degree an hour, parallax adds up to a degree).
    #[test]
    fn moon_is_opposite_the_sun_at_full_moon() {
        let when = unix(2026, 11, 24, 14, 53);
        let d = (ecliptic_longitude(equatorial(Body::Moon, when, 0.0, 0.0)) - ecliptic_longitude(sun_equatorial(when))).rem_euclid(360.0);
        assert!((d - 180.0).abs() < 2.5, "longitude difference {d:.1}");
    }

    #[test]
    fn the_bright_planets_are_where_the_late_november_sky_has_them() {
        let when = unix(2026, 11, 22, 3, 0);
        let (ra, dec) = equatorial(Body::Saturn, when, 25.0, -80.0);
        assert!((0.0..2.5).contains(&ra) || ra > 23.5, "Saturn RA {ra:.2} h"); // Pisces to Aries
        assert!((-5.0..12.0).contains(&dec), "Saturn Dec {dec:.1}");
        let (ra, dec) = equatorial(Body::Jupiter, when, 25.0, -80.0);
        assert!((9.0..12.0).contains(&ra) && (5.0..20.0).contains(&dec), "Jupiter {ra:.2} h {dec:.1}"); // Leo
    }

    #[test]
    fn ids_round_trip() {
        for b in Body::ALL {
            assert_eq!(Body::from_id(b.id()), Some(b));
        }
        assert_eq!(Body::from_id("sun"), None);
    }
}
