//! Simulated camera: a limb-darkened solar disc displaced by the shared
//! [`SimSky`] (driven by the simulated mount), a vignetted flat field, a Moon with a phase, or a small planet with seeing jitter.

use std::time::{Duration, Instant};
use rustflare_core::camera::*;
use rustflare_core::sim::SimSky;
use rustflare_core::{Error, Frame, PixelFormat, Result};

const W: u32 = 1280;
const H: u32 = 960;
const DISC_RADIUS: f64 = 300.0;
const CTRL_SCENE: &str = "sim_scene";
const CTRL_ZOOM: &str = "sim_zoom";
const CTRL_DEFOCUS: &str = "sim_defocus";
const CTRL_PHASE: &str = "sim_phase";
const CTRL_SEEING: &str = "sim_seeing";
/// Scene 3: the planet's radius at zoom 1, pixels.
const PLANET_RADIUS: f64 = 25.0;
const PIXEL_SIZE_UM: f64 = 3.75;

pub struct SimulatorDriver {
    sky: SimSky,
}

impl SimulatorDriver {
    pub fn new(sky: SimSky) -> Self { SimulatorDriver { sky } }
}

impl CameraDriver for SimulatorDriver {
    fn name(&self) -> &'static str { "simulator" }

    fn enumerate(&self) -> Result<Vec<CameraInfo>> {
        Ok(vec![
            CameraInfo { driver: "simulator".into(), id: "sim0".into(), name: "Simulated solar camera".into() },
            CameraInfo { driver: "simulator".into(), id: "sim_osc".into(), name: "Simulated colour camera (RGGB)".into() },
            CameraInfo { driver: "simulator".into(), id: "sim_cooled".into(), name: "Simulated cooled colour camera (RGGB)".into() },
        ])
    }

    fn open(&self, id: &str) -> Result<Box<dyn Camera>> {
        if !["sim0", "sim_osc", "sim_cooled"].contains(&id) {
            return Err(Error::NotFound(format!("simulator camera {id}")));
        }
        Ok(Box::new(SimCamera { colour: id != "sim0", cooler: (id == "sim_cooled").then(SimCooler::new), sky: self.sky.clone(), exposure_us: 2000.0, gain: 100.0, offset: 10.0, high_bit: true, scene: 0, roi: None, zoom: 1.0, defocus: 0.0, phase: 0.5, seeing: 0.0, rng: 0x9E3779B97F4A7C15 }))
    }
}

/// A thermoelectric cooler that moves the sensor temperature towards the target at 1 degree a second (fast, so tests do not wait).
struct SimCooler {
    on: bool,
    target: f64,
    fan: bool,
    heater: bool,
    temp: f64,
    last: Instant,
}

const SIM_AMBIENT_C: f64 = 22.0;
const SIM_COOLER_RANGE: (f64, f64) = (-40.0, 30.0);

impl SimCooler {
    fn new() -> Self { SimCooler { on: false, target: -10.0, fan: true, heater: false, temp: SIM_AMBIENT_C, last: Instant::now() } }

    fn step(&mut self) {
        let dt = self.last.elapsed().as_secs_f64();
        self.last = Instant::now();
        let goal = if self.on { self.target.max(SIM_AMBIENT_C - 35.0) } else { SIM_AMBIENT_C };
        let d = (goal - self.temp).clamp(-dt, dt);
        self.temp += d;
    }

    fn power(&self) -> f64 {
        if !self.on { return 0.0; }
        // the further the sensor is below ambient, the harder the cooler works to hold it
        (((SIM_AMBIENT_C - self.temp) / 35.0).clamp(0.0, 1.0) * 100.0).round()
    }
}

struct SimCamera {
    cooler: Option<SimCooler>,
    /// Simulated one-shot-colour sensor (RGGB mosaic of a warm-white Sun).
    colour: bool,
    sky: SimSky,
    exposure_us: f64,
    gain: f64,
    offset: f64,
    high_bit: bool,
    scene: u8,
    roi: Option<rustflare_core::Roi>,
    /// Disc radius multiplier (1 = 300 px). Above ~1.6 the disc overflows the frame, like an H-alpha scope.
    zoom: f64,
    /// Simulated focus error: edge softness in pixels (0 = sharp). Detail contrast drops with it too.
    defocus: f64,
    /// Scene 2 (Moon): 0 new, 0.5 full, 1 new again (waxing before 0.5).
    phase: f64,
    /// Scene 3 (planet): random displacement of the whole picture each frame, pixels (the seeing).
    seeing: f64,
    rng: u64,
}

impl SimCamera {
    fn noise(&mut self) -> f64 {
        // xorshift64*, mapped to roughly [-1, 1]
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        let r = self.rng.wrapping_mul(0x2545F4914F6CDD1D) >> 11;
        r as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
    }

    /// Linear signal scale: 1.0 means full-scale for the brightest pixel at this setting.
    fn level(&self) -> f64 {
        (self.exposure_us / 5000.0) * 10f64.powf(self.gain / 200.0) * 0.4
    }
}

impl Camera for SimCamera {
    fn info(&self) -> CameraInfo {
        CameraInfo { driver: "simulator".into(), id: if self.cooler.is_some() { "sim_cooled" } else if self.colour { "sim_osc" } else { "sim0" }.into(), name: if self.cooler.is_some() { "Simulated cooled colour camera (RGGB)" } else if self.colour { "Simulated colour camera (RGGB)" } else { "Simulated solar camera" }.into() }
    }

    fn bayer(&self) -> Option<rustflare_core::BayerPattern> { self.colour.then_some(rustflare_core::BayerPattern::Rggb) }

    fn controls(&mut self) -> Result<Vec<ControlDesc>> {
        let c = |id: &str, label: &str, min, max, step, value, unit: Option<&str>, log| ControlDesc {
            id: id.into(), label: label.into(), min, max, step, value, unit: unit.map(String::from), log,
        };
        Ok(vec![
            c(CTRL_EXPOSURE_US, "Exposure", 50.0, 2_000_000.0, 50.0, self.exposure_us, Some("µs"), true),
            c(CTRL_GAIN, "Gain", 0.0, 400.0, 1.0, self.gain, None, false),
            c(CTRL_OFFSET, "Offset", 0.0, 100.0, 1.0, self.offset, None, false),
            c(CTRL_HIGH_BIT_DEPTH, "16-bit output", 0.0, 1.0, 1.0, self.high_bit as u8 as f64, None, false),
            c(CTRL_SCENE, "Sim scene (0 sun, 1 flat, 2 Moon, 3 planet)", 0.0, 3.0, 1.0, self.scene as f64, None, false),
            c(CTRL_ZOOM, "Sim disc size (×300 px)", 0.5, 4.0, 0.1, self.zoom, None, false),
            c(CTRL_DEFOCUS, "Sim defocus (px)", 0.0, 30.0, 0.5, self.defocus, None, false),
            c(CTRL_PHASE, "Sim Moon phase (0 new, 0.5 full)", 0.0, 1.0, 0.05, self.phase, None, false),
            c(CTRL_SEEING, "Sim seeing jitter (px)", 0.0, 10.0, 0.5, self.seeing, None, false),
        ])
    }

    fn set_control(&mut self, id: &str, value: f64) -> Result<()> {
        match id {
            CTRL_EXPOSURE_US => self.exposure_us = value.clamp(50.0, 2_000_000.0),
            CTRL_GAIN => self.gain = value.clamp(0.0, 400.0),
            CTRL_OFFSET => self.offset = value.clamp(0.0, 100.0),
            CTRL_HIGH_BIT_DEPTH => self.high_bit = value >= 0.5,
            CTRL_SCENE => {
                let scene = value.round().clamp(0.0, 3.0) as u8;
                if scene >= 2 && scene != self.scene { self.sky.recentre(); } // the Moon and the planet start in the middle of the frame
                self.scene = scene;
            }
            CTRL_PHASE => self.phase = value.clamp(0.0, 1.0),
            CTRL_SEEING => self.seeing = value.clamp(0.0, 10.0),
            CTRL_ZOOM => self.zoom = value.clamp(0.5, 4.0),
            CTRL_DEFOCUS => self.defocus = value.clamp(0.0, 30.0),
            _ => return Err(Error::Invalid(format!("unknown control {id}"))),
        }
        Ok(())
    }

    fn capture(&mut self, _timeout: Duration) -> Result<Frame> {
        let start = Instant::now();
        let off = self.sky.offset();
        let (mut cx, mut cy) = (W as f64 / 2.0 + off.x, H as f64 / 2.0 + off.y);
        if self.scene == 3 && self.seeing > 0.0 {
            // the seeing moves the whole picture a little, differently every frame
            cx += self.noise() * self.seeing;
            cy += self.noise() * self.seeing;
        }
        let level = self.level();
        let radius = if self.scene == 3 { PLANET_RADIUS * self.zoom } else { DISC_RADIUS * self.zoom };
        let full = if self.high_bit { 65535.0 } else { 255.0 };
        let black = self.offset / 100.0 * 0.02;
        let bpp = if self.high_bit { 2 } else { 1 };
        let mut data = Vec::with_capacity(W as usize * H as usize * bpp);

        for y in 0..H {
            for x in 0..W {
                let signal = if self.scene >= 2 {
                    let (dx, dy) = (x as f64 - cx, y as f64 - cy);
                    if self.scene == 2 { moon_signal(dx, dy, radius, self.phase, level) } else { planet_signal(dx, dy, radius, level) }
                } else if self.scene == 0 {
                    let (dx, dy) = (x as f64 - cx, y as f64 - cy);
                    let r2 = (dx * dx + dy * dy) / (radius * radius);
                    let blur = self.defocus;
                    if blur > 0.1 && r2 < (1.0 + 8.0 * blur / radius).powi(2) {
                        // out of focus: the limb is a soft ramp and the surface detail loses contrast
                        let rr = r2.sqrt() * radius;
                        let edge = 1.0 / (1.0 + ((rr - radius) / (blur * 0.5)).exp());
                        let mu = (1.0 - r2.min(0.999)).sqrt();
                        let t = ((dx / 23.0).sin() * (dy / 31.0).cos() + 1.0) / 2.0;
                        let plage = (-(((dx - 0.3 * radius) / 40.0).powi(2) + ((dy + 0.2 * radius) / 30.0).powi(2))).exp();
                        let detail = 1.0 / (1.0 + blur / 4.0);
                        let structure = 0.55 + (0.25 * t * t + 0.2 * plage) * detail;
                        ((1.0 - 0.4 * (1.0 - mu)) * structure * edge + 0.002) * level
                    } else if r2 < 1.0 {
                        // limb darkening, mu = sqrt(1 - r^2), times H-alpha-like structure: smooth, attached to
                        // the disc, with dark filaments and a bright plage patch
                        let mu = (1.0 - r2).sqrt();
                        let t = ((dx / 23.0).sin() * (dy / 31.0).cos() + 1.0) / 2.0;
                        let plage = (-(((dx - 0.3 * radius) / 40.0).powi(2) + ((dy + 0.2 * radius) / 30.0).powi(2))).exp();
                        let structure = 0.55 + 0.25 * t * t + 0.2 * plage;
                        (1.0 - 0.4 * (1.0 - mu)) * structure * level
                    } else {
                        0.002 * level
                    }
                } else {
                    // vignetted flat: 100% at centre, ~70% in corners
                    let (dx, dy) = ((x as f64 - W as f64 / 2.0) / (W as f64 / 2.0), (y as f64 - H as f64 / 2.0) / (H as f64 / 2.0));
                    (1.0 - 0.15 * (dx * dx + dy * dy)) * level
                };
                // colour sensor: each photosite sees one channel of a warm Sun (R 1.0, G 0.8, B 0.55)
                let tint = match self.scene { 2 => [1.0, 0.95, 0.95, 0.9], 3 => [1.0, 0.7, 0.7, 0.4], _ => [1.0, 0.8, 0.8, 0.55] };
                let signal = if self.colour { signal * tint[((y & 1) * 2 + (x & 1)) as usize] } else { signal };
                let v = (signal + black + self.noise() * 0.004).clamp(0.0, 1.0);
                let q = (v * full).round() as u32;
                if self.high_bit { data.extend_from_slice(&(q as u16).to_le_bytes()); } else { data.push(q as u8); }
            }
        }

        // pace like a real sensor
        let frame_time = Duration::from_micros(self.exposure_us as u64).max(Duration::from_millis(33));
        if let Some(rest) = frame_time.checked_sub(start.elapsed()) {
            std::thread::sleep(rest);
        }
        let full_frame = Frame::new(W, H, if self.high_bit { PixelFormat::Mono16 } else { PixelFormat::Mono8 }, data);
        Ok(match self.roi {
            None => full_frame,
            Some(r) => {
                let bpp = full_frame.format.bytes_per_pixel();
                let mut d = Vec::with_capacity(r.width as usize * r.height as usize * bpp);
                for y in r.y..r.y + r.height {
                    let a = (y as usize * W as usize + r.x as usize) * bpp;
                    d.extend_from_slice(&full_frame.data[a..a + r.width as usize * bpp]);
                }
                Frame::new(r.width, r.height, full_frame.format, d)
            }
        })
    }

    fn sensor_size(&self) -> Option<(u32, u32)> { Some((W, H)) }

    fn roi(&self) -> Option<rustflare_core::Roi> { Some(self.roi.unwrap_or(rustflare_core::Roi { x: 0, y: 0, width: W, height: H })) }

    fn set_roi(&mut self, roi: rustflare_core::Roi) -> Result<()> {
        if roi.width == 0 || roi.height == 0 || roi.x + roi.width > W || roi.y + roi.height > H {
            return Err(rustflare_core::Error::Invalid("region outside the sensor".into()));
        }
        self.roi = if roi.width == W && roi.height == H { None } else { Some(roi) };
        Ok(())
    }

    fn temperature_c(&mut self) -> Option<f64> {
        match self.cooler.as_mut() {
            Some(c) => { c.step(); Some((c.temp * 10.0).round() / 10.0) }
            None => Some(24.5),
        }
    }

    fn cooler(&mut self) -> Option<CoolerStatus> {
        let c = self.cooler.as_mut()?;
        c.step();
        Some(CoolerStatus {
            on: c.on, target_c: Some(c.target), target_min_c: Some(SIM_COOLER_RANGE.0), target_max_c: Some(SIM_COOLER_RANGE.1),
            power_pct: Some(c.power()), fan: Some(c.fan), heater: Some(c.heater),
        })
    }

    fn set_cooler(&mut self, req: &CoolerRequest) -> Result<()> {
        let c = self.cooler.as_mut().ok_or_else(|| Error::Unsupported("cooler on this camera".into()))?;
        c.step();
        if let Some(t) = req.target_c { c.target = t.round().clamp(SIM_COOLER_RANGE.0, SIM_COOLER_RANGE.1); }
        if let Some(on) = req.on { c.on = on; }
        if let Some(f) = req.fan { c.fan = f; }
        if let Some(h) = req.heater { c.heater = h; }
        Ok(())
    }

    fn pixel_size_um(&self) -> Option<f64> { Some(PIXEL_SIZE_UM) }
}

/// Scene 2: the Moon. A grey sphere lit from a direction set by the phase (0 new, 0.5 full): the terminator is a soft, curved edge, the
/// maria are dark blobs and a few craters are small bright rings, all fixed on the disc. Earthshine lights the dark part faintly.
fn moon_signal(dx: f64, dy: f64, radius: f64, phase: f64, level: f64) -> f64 {
    let (u, v) = (dx / radius, dy / radius);
    let r2 = u * u + v * v;
    if r2 >= 1.0 {
        return 0.002 * level;
    }
    let w = (1.0 - r2).sqrt();
    let ph = phase * std::f64::consts::TAU;
    // light direction: facing us at the full Moon (phase 0.5), from the side at the quarters
    let lit = (u * ph.sin() + w * -ph.cos()).clamp(-1.0, 1.0);
    let sunlit = ((lit + 0.02) / 0.06).clamp(0.0, 1.0); // soft terminator, a few pixels wide
    let blob = |cx: f64, cy: f64, s: f64| (-(((u - cx) / s).powi(2) + ((v - cy) / s).powi(2))).exp();
    let maria = 0.35 * blob(-0.35, -0.25, 0.28) + 0.3 * blob(0.15, -0.35, 0.22) + 0.25 * blob(-0.1, 0.3, 0.3) + 0.2 * blob(0.45, 0.1, 0.15);
    let crater = |cx: f64, cy: f64, rr: f64| { let d = ((u - cx).powi(2) + (v - cy).powi(2)).sqrt(); 0.25 * (-((d - rr) / 0.012).powi(2)).exp() };
    let craters = crater(0.3, 0.45, 0.05) + crater(-0.55, 0.2, 0.035) + crater(0.1, -0.7, 0.06) + crater(-0.2, 0.6, 0.025);
    let texture = 0.04 * ((dx / 7.0).sin() * (dy / 5.0).cos());
    let albedo = (0.75 - maria + craters + texture).clamp(0.05, 1.0);
    (albedo * sunlit * (0.85 + 0.15 * w) + 0.01 * albedo) * level + 0.002 * level
}

/// Scene 3: a planet a few tens of pixels across: a slightly oblate disc, limb darkened, with two dark bands, a dark spot, and two
/// small moons (3 px) nearby.
fn planet_signal(dx: f64, dy: f64, radius: f64, level: f64) -> f64 {
    let moon = |mx: f64, my: f64| { let d = ((dx - mx * radius).powi(2) + (dy - my * radius).powi(2)).sqrt(); 0.25 * (-(d / (0.12 * radius)).powi(2)).exp() };
    let moons = moon(3.6, -1.2) + moon(-4.8, 1.8);
    let (u, v) = (dx / radius, dy / (0.94 * radius)); // oblate
    let r2 = u * u + v * v;
    if r2 >= 1.0 {
        // a soft edge one pixel wide, so the disc is not aliased
        let d = (r2.sqrt() - 1.0) * radius;
        return (0.002 + moons + 0.4 * (1.0 - d).max(0.0)) * level;
    }
    let mu = (1.0 - r2).sqrt();
    let bands = 0.75 + 0.25 * (v * 5.0).sin() - 0.2 * (-((v + 0.35) / 0.12).powi(2)).exp();
    let spot = 0.3 * (-(((u - 0.35) / 0.18).powi(2) + ((v - 0.3) / 0.1).powi(2))).exp();
    ((1.0 - 0.45 * (1.0 - mu)) * (bands - spot) * 0.9 + 0.002 + moons) * level
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_simulated_cooler_cools_and_only_the_cooled_camera_has_one() {
        let d = SimulatorDriver::new(SimSky::new());
        assert!(d.open("sim0").unwrap().cooler().is_none());
        let mut cam = d.open("sim_cooled").unwrap();
        let st = cam.cooler().unwrap();
        assert!(!st.on && st.power_pct == Some(0.0));
        cam.set_cooler(&CoolerRequest { on: Some(true), target_c: Some(-500.0), ..Default::default() }).unwrap();
        assert_eq!(cam.cooler().unwrap().target_c, Some(-40.0));
        std::thread::sleep(Duration::from_millis(1200));
        assert!(cam.temperature_c().unwrap() < 22.0);
        assert!(cam.cooler().unwrap().power_pct.unwrap() > 0.0);
    }

    #[test]
    fn exposure_changes_brightness() {
        let d = SimulatorDriver::new(SimSky::new());
        let mut cam = d.open("sim0").unwrap();
        cam.set_control(CTRL_SCENE, 1.0).unwrap();
        cam.set_control(CTRL_EXPOSURE_US, 1000.0).unwrap();
        let dark = cam.capture(Duration::from_secs(1)).unwrap().mean_fraction();
        cam.set_control(CTRL_EXPOSURE_US, 4000.0).unwrap();
        let bright = cam.capture(Duration::from_secs(1)).unwrap().mean_fraction();
        assert!(bright > dark * 2.0, "{dark} vs {bright}");
    }

    fn scene(id: u8, controls: &[(&str, f64)]) -> Frame {
        let sky = SimSky::new();
        sky.set_tracking(true); // the free-running sky drifts 15 px/s: the tests look at where things are
        let d = SimulatorDriver::new(sky);
        let mut cam = d.open("sim0").unwrap();
        cam.set_control(CTRL_SCENE, id as f64).unwrap();
        cam.set_control(CTRL_EXPOSURE_US, 2000.0).unwrap();
        for (k, v) in controls { cam.set_control(k, *v).unwrap(); }
        cam.capture(Duration::from_secs(1)).unwrap()
    }

    /// Fraction of the frame above half of the brightest pixel, and the centroid of those pixels.
    fn bright_part(f: &Frame) -> (f64, (f64, f64)) {
        let peak = (0..f.height).flat_map(|y| (0..f.width).map(move |x| (x, y))).map(|(x, y)| f.pixel(x, y)).max().unwrap() as f64;
        let (mut n, mut sx, mut sy) = (0.0, 0.0, 0.0);
        for y in 0..f.height { for x in 0..f.width { if f.pixel(x, y) as f64 > peak * 0.5 { n += 1.0; sx += x as f64; sy += y as f64; } } }
        (n / (f.width as f64 * f.height as f64), (sx / n, sy / n))
    }

    #[test]
    fn the_moon_scene_has_a_phase() {
        let full = bright_part(&scene(2, &[(CTRL_PHASE, 0.5)]));
        let quarter = bright_part(&scene(2, &[(CTRL_PHASE, 0.25)]));
        let new = scene(2, &[(CTRL_PHASE, 0.0)]);
        // a full Moon is a disc of radius 300 (23 % of the frame); at the quarter about half of it is lit, on one side of the centre
        assert!((0.15..0.28).contains(&full.0), "full {:?}", full);
        assert!(quarter.0 < full.0 * 0.7 && quarter.0 > full.0 * 0.25, "quarter {:?} vs full {:?}", quarter, full);
        assert!((full.1.0 - 640.0).abs() < 8.0, "the full Moon is centred: {:?}", full);
        assert!((quarter.1.0 - 640.0).abs() > 40.0, "the lit half is off the centre: {:?}", quarter);
        // the new Moon is nearly dark (earthshine only)
        let peak = (0..new.height).flat_map(|y| (0..new.width).map(move |x| (x, y))).map(|(x, y)| new.pixel(x, y)).max().unwrap() as f64 / 65535.0;
        assert!(peak < 0.12, "new Moon peak {peak}");
    }

    #[test]
    fn the_planet_scene_is_a_small_disc_with_seeing() {
        let f = scene(3, &[]);
        let (frac, (cx, cy)) = bright_part(&f);
        assert!(frac < 0.005 && frac > 0.0005, "a ~25 px planet is a small part of the frame: {frac}");
        assert!((cx - 640.0).abs() < 5.0 && (cy - 480.0).abs() < 5.0, "{cx},{cy}");
        // with seeing the picture moves from frame to frame
        let sky = SimSky::new();
        sky.set_tracking(true);
        let d = SimulatorDriver::new(sky);
        let mut cam = d.open("sim0").unwrap();
        cam.set_control(CTRL_SCENE, 3.0).unwrap();
        cam.set_control(CTRL_SEEING, 3.0).unwrap();
        let mut xs = vec![];
        for _ in 0..6 { xs.push(bright_part(&cam.capture(Duration::from_secs(1)).unwrap()).1.0); }
        let spread = xs.iter().cloned().fold(f64::MIN, f64::max) - xs.iter().cloned().fold(f64::MAX, f64::min);
        assert!(spread > 1.0 && spread < 12.0, "seeing spread {spread} {xs:?}");
    }
}
