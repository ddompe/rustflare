//! Master dark and flat frames, built here from the raw dark and flat recordings (SER) and applied to every frame before it is
//! judged or stacked. Nothing is read from derived files such as the recorder's PNG masters: the raw frames are the product.

use crate::ser::Ser;
use std::path::Path;

#[derive(Default, Clone)]
pub struct Calibration {
    pub width: usize,
    pub height: usize,
    pub dark: Option<Vec<f32>>,
    /// Flat normalised to a mean of 1 (never below 0.1, so dust and dead pixels cannot blow the image up).
    pub flat: Option<Vec<f32>>,
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
    /// `dark` and `flat` are raw recordings (SER) of dark frames (cap on) and of a flat field. The flat has the dark taken off
    /// (when a dark is given) before it is normalised to a mean of 1.
    pub fn load(dark: Option<&Path>, flat: Option<&Path>) -> Result<Calibration, String> {
        let mut c = Calibration::default();
        if let Some(p) = dark {
            let (w, h, d) = mean_of(p)?;
            (c.width, c.height, c.dark) = (w, h, Some(d));
        }
        if let Some(p) = flat {
            let (w, h, mut f) = mean_of(p)?;
            if let Some(d) = &c.dark {
                if (c.width, c.height) != (w, h) {
                    return Err("the dark and the flat differ in size".into());
                }
                for (v, d) in f.iter_mut().zip(d) {
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
            c.flat = Some(f);
        }
        Ok(c)
    }

    /// A raw frame to calibrated floats: (raw - dark) / flat.
    pub fn apply(&self, raw: &[u16]) -> Vec<f32> {
        let mut out: Vec<f32> = raw.iter().map(|&v| v as f32).collect();
        if let Some(d) = &self.dark {
            for (o, d) in out.iter_mut().zip(d) {
                *o = (*o - d).max(0.0);
            }
        }
        if let Some(f) = &self.flat {
            for (o, f) in out.iter_mut().zip(f) {
                *o /= f;
            }
        }
        out
    }
}
