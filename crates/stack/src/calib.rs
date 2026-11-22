//! Master dark and flat frames (the 16-bit grey PNGs the recorder writes) applied to every frame before it is judged or stacked.

use std::path::Path;

#[derive(Default, Clone)]
pub struct Calibration {
    pub width: usize,
    pub height: usize,
    pub dark: Option<Vec<f32>>,
    /// Flat normalised to a mean of 1 (never below 0.1, so dust and dead pixels cannot blow the image up).
    pub flat: Option<Vec<f32>>,
}

fn read_png(path: &Path) -> Result<(usize, usize, Vec<f32>), String> {
    let dec = png::Decoder::new(std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?);
    let mut r = dec.read_info().map_err(|e| format!("{}: {e}", path.display()))?;
    let mut buf = vec![0u8; r.output_buffer_size()];
    let info = r.next_frame(&mut buf).map_err(|e| format!("{}: {e}", path.display()))?;
    if info.color_type != png::ColorType::Grayscale {
        return Err(format!("{}: expected a grey image", path.display()));
    }
    let data: Vec<f32> = match info.bit_depth {
        png::BitDepth::Sixteen => buf[..info.buffer_size()].chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]]) as f32).collect(),
        png::BitDepth::Eight => buf[..info.buffer_size()].iter().map(|&b| b as f32 * 257.0).collect(),
        _ => return Err(format!("{}: only 8 and 16-bit images are supported", path.display())),
    };
    Ok((info.width as usize, info.height as usize, data))
}

impl Calibration {
    pub fn load(dark: Option<&Path>, flat: Option<&Path>) -> Result<Calibration, String> {
        let mut c = Calibration::default();
        if let Some(p) = dark {
            let (w, h, d) = read_png(p)?;
            (c.width, c.height, c.dark) = (w, h, Some(d));
        }
        if let Some(p) = flat {
            let (w, h, mut f) = read_png(p)?;
            if c.dark.is_some() && (c.width, c.height) != (w, h) {
                return Err("the dark and the flat differ in size".into());
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
