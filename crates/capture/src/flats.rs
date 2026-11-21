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
}
