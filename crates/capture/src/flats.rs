//! Flat-field capture logic: find an exposure that puts the mean level at a
//! target fraction of full scale, then average the frames into a master flat.

use rustflare_core::{Error, Frame, PixelFormat, Result};

#[derive(Clone, Copy, Debug)]
pub struct FlatTarget {
    /// Desired mean level as a fraction of full scale (typically 0.4–0.6).
    pub level: f64,
    pub tolerance: f64,
    pub min_exposure_us: f64,
    pub max_exposure_us: f64,
    pub max_iterations: u32,
}

impl Default for FlatTarget {
    fn default() -> Self {
        FlatTarget { level: 0.5, tolerance: 0.05, min_exposure_us: 50.0, max_exposure_us: 2_000_000.0, max_iterations: 10 }
    }
}

#[derive(Debug, PartialEq)]
pub enum FlatDecision {
    Accept,
    SetExposure(f64),
    GiveUp(String),
}

impl FlatTarget {
    /// Decide what to do after measuring a frame taken at `exposure_us`.
    /// Assumes a linear sensor response, which holds well away from saturation.
    pub fn decide(&self, iteration: u32, exposure_us: f64, mean: f64, saturated: f64) -> FlatDecision {
        let clipped = saturated > 0.001 || mean > 0.98;
        if !clipped && (mean - self.level).abs() <= self.tolerance {
            return FlatDecision::Accept;
        }
        if iteration >= self.max_iterations {
            return FlatDecision::GiveUp(format!("did not converge after {iteration} attempts (mean {:.0}%)", mean * 100.0));
        }
        // A clipped frame says nothing about the true level: back off hard.
        let wanted = if clipped { exposure_us * 0.5 } else { exposure_us * self.level / mean.max(1e-3) };
        let next = wanted.clamp(self.min_exposure_us, self.max_exposure_us);
        if (next - exposure_us).abs() < 1.0 {
            return if mean < self.level {
                FlatDecision::GiveUp(format!("too dark even at maximum exposure ({:.0}%)", mean * 100.0))
            } else {
                FlatDecision::GiveUp(format!("too bright even at minimum exposure ({:.0}%)", mean * 100.0))
            };
        }
        FlatDecision::SetExposure(next)
    }
}

/// Sums frames and produces their mean as a 16-bit frame.
pub struct FlatAccumulator {
    width: u32,
    height: u32,
    format: PixelFormat,
    sum: Vec<u64>,
    n: u32,
}

impl FlatAccumulator {
    pub fn new(width: u32, height: u32, format: PixelFormat) -> Self {
        FlatAccumulator { width, height, format, sum: vec![0; width as usize * height as usize], n: 0 }
    }

    pub fn add(&mut self, f: &Frame) -> Result<()> {
        if (f.width, f.height, f.format) != (self.width, self.height, self.format) {
            return Err(Error::Invalid("flat frame format changed mid-sequence".into()));
        }
        for (i, s) in self.sum.iter_mut().enumerate() {
            *s += match f.format {
                PixelFormat::Mono8 => f.data[i] as u64,
                PixelFormat::Mono16 => u16::from_le_bytes([f.data[2 * i], f.data[2 * i + 1]]) as u64,
            };
        }
        self.n += 1;
        Ok(())
    }

    pub fn count(&self) -> u32 { self.n }

    /// Mean image, always returned as Mono16 (8-bit input is scaled by 257).
    pub fn master(&self) -> Result<Frame> {
        if self.n == 0 {
            return Err(Error::Invalid("no flat frames accumulated".into()));
        }
        let scale = if self.format == PixelFormat::Mono8 { 257.0 } else { 1.0 };
        let mut data = Vec::with_capacity(self.sum.len() * 2);
        for s in &self.sum {
            let v = (*s as f64 / self.n as f64 * scale).round().min(65535.0) as u16;
            data.extend_from_slice(&v.to_le_bytes());
        }
        Ok(Frame::new(self.width, self.height, PixelFormat::Mono16, data))
    }
}

/// Flat-field correction: per-pixel gains (Q12 fixed point) that even out vignetting and dust shadows.
/// Applying is one integer multiply per pixel in a branch-free loop the compiler vectorises (NEON on the Pi 5).
pub struct FlatField {
    pub width: u32,
    pub height: u32,
    gain: Vec<u16>,
}

const GAIN_ONE: u32 = 4096;

impl FlatField {
    /// Gains are `mean / pixel`, limited to 0.25..8 so dead pixels and cap leaks cannot blow up.
    pub fn from_master(m: &Frame) -> Result<FlatField> {
        if m.format != PixelFormat::Mono16 {
            return Err(Error::Invalid("master flat must be 16-bit".into()));
        }
        let px: Vec<u16> = m.data.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        let (sum, n) = px.iter().filter(|v| **v > 0).fold((0f64, 0u64), |(s, n), v| (s + *v as f64, n + 1));
        if n == 0 {
            return Err(Error::Invalid("master flat is black".into()));
        }
        let mean = sum / n as f64;
        let gain = px.iter().map(|v| ((mean / (*v).max(1) as f64) * GAIN_ONE as f64).clamp(GAIN_ONE as f64 / 4.0, GAIN_ONE as f64 * 8.0) as u16).collect();
        Ok(FlatField { width: m.width, height: m.height, gain })
    }

    pub fn matches(&self, f: &Frame) -> bool {
        (f.width, f.height) == (self.width, self.height)
    }

    /// Corrects `f` in place (Mono16 or Mono8). Caller checks `matches` first.
    pub fn apply(&self, f: &mut Frame) {
        match f.format {
            PixelFormat::Mono16 => {
                for (c, g) in f.data.chunks_exact_mut(2).zip(&self.gain) {
                    let v = u16::from_le_bytes([c[0], c[1]]) as u32;
                    let o = ((v * *g as u32 + GAIN_ONE / 2) >> 12).min(65535) as u16;
                    c.copy_from_slice(&o.to_le_bytes());
                }
            }
            PixelFormat::Mono8 => {
                for (p, g) in f.data.iter_mut().zip(&self.gain) {
                    *p = ((*p as u32 * *g as u32 + GAIN_ONE / 2) >> 12).min(255) as u8;
                }
            }
        }
    }
}

/// What the preview applies, shared between the engine (sets it) and the preview thread (reads it each frame).
#[derive(Default)]
pub struct FlatSetting {
    pub field: Option<std::sync::Arc<FlatField>>,
    pub enabled: bool,
    pub name: Option<String>,
}

pub type SharedFlat = std::sync::Arc<std::sync::Mutex<FlatSetting>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converges_on_linear_sensor() {
        let t = FlatTarget::default();
        let (mut exp, mut i) = (1000.0, 0);
        // sensor: mean = exposure / 10000 (full scale at 10 ms)
        loop {
            let mean = (exp / 10_000.0f64).min(1.0);
            match t.decide(i, exp, mean, if mean >= 1.0 { 0.5 } else { 0.0 }) {
                FlatDecision::Accept => break,
                FlatDecision::SetExposure(e) => exp = e,
                FlatDecision::GiveUp(m) => panic!("{m}"),
            }
            i += 1;
        }
        assert!((exp / 10_000.0 - 0.5).abs() < 0.05);
    }

    #[test]
    fn saturation_backs_off() {
        let t = FlatTarget::default();
        assert_eq!(t.decide(0, 8000.0, 1.0, 0.9), FlatDecision::SetExposure(4000.0));
    }

    #[test]
    fn gives_up_when_too_dark_at_max() {
        let t = FlatTarget::default();
        assert!(matches!(t.decide(1, 2_000_000.0, 0.05, 0.0), FlatDecision::GiveUp(_)));
    }

    #[test]
    fn master_is_mean() {
        let mut a = FlatAccumulator::new(2, 1, PixelFormat::Mono8);
        a.add(&Frame::new(2, 1, PixelFormat::Mono8, vec![10, 20])).unwrap();
        a.add(&Frame::new(2, 1, PixelFormat::Mono8, vec![20, 40])).unwrap();
        let m = a.master().unwrap();
        assert_eq!((m.pixel(0, 0), m.pixel(1, 0)), (15 * 257, 30 * 257));
    }

    #[test]
    fn flat_field_removes_vignetting() {
        // 4x1 sensor: edges see 80% of the centre
        let flat = Frame::new(4, 1, PixelFormat::Mono16, [40000u16, 50000, 50000, 40000].iter().flat_map(|v| v.to_le_bytes()).collect());
        let ff = FlatField::from_master(&flat).unwrap();
        let mut img = Frame::new(4, 1, PixelFormat::Mono16, [20000u16, 25000, 25000, 20000].iter().flat_map(|v| v.to_le_bytes()).collect());
        ff.apply(&mut img);
        let out: Vec<u16> = (0..4).map(|x| img.pixel(x, 0)).collect();
        let spread = (*out.iter().max().unwrap() as f64 - *out.iter().min().unwrap() as f64) / 25000.0;
        assert!(spread < 0.01, "{out:?}");
        // 8-bit works and never overflows
        let mut b = Frame::new(4, 1, PixelFormat::Mono8, vec![255, 255, 255, 255]);
        ff.apply(&mut b);
        assert!(b.data.iter().all(|v| *v <= 255));
    }
}
