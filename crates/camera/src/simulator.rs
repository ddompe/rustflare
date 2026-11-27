//! Simulated camera: a limb-darkened solar disc displaced by the shared
//! [`SimSky`] (driven by the simulated mount), or a vignetted flat field.

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
        Ok(Box::new(SimCamera { colour: id != "sim0", cooler: (id == "sim_cooled").then(SimCooler::new), sky: self.sky.clone(), exposure_us: 2000.0, gain: 100.0, offset: 10.0, high_bit: true, scene: 0, roi: None, zoom: 1.0, defocus: 0.0, rng: 0x9E3779B97F4A7C15 }))
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
            c(CTRL_SCENE, "Sim scene (0 sun, 1 flat)", 0.0, 1.0, 1.0, self.scene as f64, None, false),
            c(CTRL_ZOOM, "Sim disc size (×300 px)", 0.5, 4.0, 0.1, self.zoom, None, false),
            c(CTRL_DEFOCUS, "Sim defocus (px)", 0.0, 30.0, 0.5, self.defocus, None, false),
        ])
    }

    fn set_control(&mut self, id: &str, value: f64) -> Result<()> {
        match id {
            CTRL_EXPOSURE_US => self.exposure_us = value.clamp(50.0, 2_000_000.0),
            CTRL_GAIN => self.gain = value.clamp(0.0, 400.0),
            CTRL_OFFSET => self.offset = value.clamp(0.0, 100.0),
            CTRL_HIGH_BIT_DEPTH => self.high_bit = value >= 0.5,
            CTRL_SCENE => self.scene = if value >= 0.5 { 1 } else { 0 },
            CTRL_ZOOM => self.zoom = value.clamp(0.5, 4.0),
            CTRL_DEFOCUS => self.defocus = value.clamp(0.0, 30.0),
            _ => return Err(Error::Invalid(format!("unknown control {id}"))),
        }
        Ok(())
    }

    fn capture(&mut self, _timeout: Duration) -> Result<Frame> {
        let start = Instant::now();
        let off = self.sky.offset();
        let (cx, cy) = (W as f64 / 2.0 + off.x, H as f64 / 2.0 + off.y);
        let level = self.level();
        let radius = DISC_RADIUS * self.zoom;
        let full = if self.high_bit { 65535.0 } else { 255.0 };
        let black = self.offset / 100.0 * 0.02;
        let bpp = if self.high_bit { 2 } else { 1 };
        let mut data = Vec::with_capacity(W as usize * H as usize * bpp);

        for y in 0..H {
            for x in 0..W {
                let signal = if self.scene == 0 {
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
                let signal = if self.colour { signal * [1.0, 0.8, 0.8, 0.55][((y & 1) * 2 + (x & 1)) as usize] } else { signal };
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
}
