//! Master dark and flat frames, built here from the raw dark and flat recordings (SER) and applied to every frame before it is
//! judged or stacked. Nothing is read from derived files such as the recorder's PNG masters: the raw frames are the product.

use crate::ser::Ser;
use std::path::Path;
use std::sync::Arc;

#[derive(Default, Clone)]
pub struct Calibration {
    pub width: usize,
    pub height: usize,
    pub dark: Option<Arc<Vec<f32>>>,
    /// Flat normalised to a mean of 1 (never below 0.1, so dust and dead pixels cannot blow the image up).
    pub flat: Option<Arc<Vec<f32>>>,
}

/// The mean of all the frames of a raw SER recording, in float (no rounding, no intermediate file format).
fn mean_of(path: &Path) -> Result<(usize, usize, Vec<f32>), String> {
    let mut ser = Ser::open(path).map_err(|e| e.to_string())?;
    let (w, h, n) = (ser.width, ser.height, ser.frames());
    if n == 0 {
        return Err(format!("{}: no frames", path.display()));
    }
    let mut sum = vec![0f64; w * h];
    for i in 0..n {
        for (s, v) in sum.iter_mut().zip(ser.read_frame(i).map_err(|e| e.to_string())?) {
            *s += v as f64;
        }
    }
    Ok((w, h, sum.into_iter().map(|v| (v / n as f64) as f32).collect()))
}

impl Calibration {
    /// A flat from raw frames with the given master dark taken off first.
    pub fn load_with_dark(flat: &Path, dark: Arc<Vec<f32>>) -> Result<Calibration, String> {
        let (w, h, mut f) = mean_of(flat)?;
        if dark.len() != w * h {
            return Err("the dark and the flat differ in size".into());
        }
        for (v, d) in f.iter_mut().zip(dark.iter()) {
            *v = (*v - d).max(0.0);
        }
        let mean = f.iter().map(|&v| v as f64).sum::<f64>() / f.len() as f64;
        if mean <= 0.0 {
            return Err("the flat is black".into());
        }
        for v in &mut f {
            *v = ((*v as f64 / mean) as f32).max(0.1);
        }
        Ok(Calibration { width: w, height: h, dark: None, flat: Some(Arc::new(f)) })
    }

    /// `dark` and `flat` are raw recordings (SER) of dark frames (cap on) and of a flat field. The flat has the dark taken off
    /// (when a dark is given) before it is normalised to a mean of 1.
    pub fn load(dark: Option<&Path>, flat: Option<&Path>) -> Result<Calibration, String> {
        let mut c = Calibration::default();
        if let Some(p) = dark {
            let (w, h, d) = mean_of(p)?;
            (c.width, c.height, c.dark) = (w, h, Some(Arc::new(d)));
        }
        if let Some(p) = flat {
            let (w, h, mut f) = mean_of(p)?;
            if let Some(d) = &c.dark {
                if (c.width, c.height) != (w, h) {
                    return Err("the dark and the flat differ in size".into());
                }
                for (v, d) in f.iter_mut().zip(d.iter()) {
                    *v = (*v - d).max(0.0);
                }
            }
            (c.width, c.height) = (w, h);
            let mean = f.iter().map(|&v| v as f64).sum::<f64>() / f.len() as f64;
            if mean <= 0.0 {
                return Err("the flat is black".into());
            }
            for v in &mut f {
                *v = ((*v as f64 / mean) as f32).max(0.1);
            }
            c.flat = Some(Arc::new(f));
        }
        Ok(c)
    }

    /// A raw frame to calibrated floats: (raw - dark) / flat.
    pub fn apply(&self, raw: &[u16]) -> Vec<f32> {
        let mut out: Vec<f32> = raw.iter().map(|&v| v as f32).collect();
        if let Some(d) = &self.dark {
            for (o, d) in out.iter_mut().zip(d.iter()) {
                *o = (*o - d).max(0.0);
            }
        }
        if let Some(f) = &self.flat {
            for (o, f) in out.iter_mut().zip(f.iter()) {
                *o /= f;
            }
        }
        out
    }
}


/// What a recording was taken with, from the sidecar JSON next to it (`name.json`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Settings {
    pub exposure_us: Option<f64>,
    pub gain: Option<f64>,
}

impl Settings {
    pub fn from_sidecar(recording: &Path) -> Settings {
        let Ok(text) = std::fs::read_to_string(recording.with_extension("json")) else { return Settings::default() };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { return Settings::default() };
        let num = |a: &str, b: &str| v.get(a).and_then(|c| c.get(b)).and_then(|x| x.as_f64());
        Settings { exposure_us: num("controls", "exposure_us").or_else(|| num("details", "exposure_us")), gain: num("controls", "gain").or_else(|| num("details", "gain")) }
    }
}

/// Several master darks (one per exposure) and a flat, to pick the calibration that fits a recording.
#[derive(Default)]
pub struct CalSet {
    darks: Vec<(Settings, Arc<Vec<f32>>)>,
    pub flat: Option<Calibration>,
}

impl CalSet {
    /// `darks` are raw dark recordings (each with its sidecar for the exposure and gain), `flat` a raw flat recording.
    pub fn load(darks: &[std::path::PathBuf], flat: Option<&Path>) -> Result<CalSet, String> {
        let mut set = CalSet::default();
        for d in darks {
            let c = Calibration::load(Some(d), None)?;
            set.darks.push((Settings::from_sidecar(d), c.dark.unwrap()));
        }
        if let Some(f) = flat {
            let fs = Settings::from_sidecar(f);
            let dark_for_flat = set.darks.iter().find(|(s, _)| matches(*s, fs)).map(|(_, d)| d.clone());
            set.flat = Some(match dark_for_flat {
                Some(d) => Calibration::load_with_dark(f, d)?,
                None => Calibration::load(None, Some(f))?,
            });
        }
        Ok(set)
    }

    /// The calibration for a recording with these settings: the flat, and the dark taken at the same exposure and gain if there is one.
    /// The second value says what was left out and why.
    pub fn for_settings(&self, s: Settings) -> (Calibration, Vec<String>) {
        let mut c = self.flat.clone().unwrap_or_default();
        let mut notes = Vec::new();
        if !self.darks.is_empty() {
            match self.darks.iter().find(|(d, _)| matches(*d, s)) {
                Some((_, d)) => c.dark = Some(d.clone()),
                None => notes.push(format!("no dark matches {:?} us at gain {:?}: not applied", s.exposure_us.map(|e| e.round()), s.gain)),
            }
        }
        (c, notes)
    }
}

/// The same exposure (within 3 %) and gain, when both are known.
fn matches(dark: Settings, rec: Settings) -> bool {
    let near = |a: Option<f64>, b: Option<f64>, tol: f64| match (a, b) { (Some(a), Some(b)) => (a - b).abs() <= tol * a.abs().max(b.abs()).max(1e-9), _ => true };
    near(dark.exposure_us, rec.exposure_us, 0.03) && near(dark.gain, rec.gain, 0.0)
}
