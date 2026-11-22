use serde::Serialize;
use std::time::SystemTime;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
pub enum PixelFormat {
    Mono8,
    /// 16 bits per pixel, little-endian in `Frame::data`.
    Mono16,
}

impl PixelFormat {
    pub fn bytes_per_pixel(self) -> usize {
        match self { PixelFormat::Mono8 => 1, PixelFormat::Mono16 => 2 }
    }
    pub fn full_scale(self) -> f64 {
        match self { PixelFormat::Mono8 => 255.0, PixelFormat::Mono16 => 65535.0 }
    }
}

/// Colour filter array layout of a one-shot-colour sensor, as seen from the top-left of the frame.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
pub enum BayerPattern {
    Rggb,
    Bggr,
    Grbg,
    Gbrg,
}

impl BayerPattern {
    /// Channel (0 = R, 1 = G, 2 = B) at the four positions of a 2x2 block: (0,0), (1,0), (0,1), (1,1).
    pub fn block(self) -> [usize; 4] {
        match self {
            BayerPattern::Rggb => [0, 1, 1, 2],
            BayerPattern::Bggr => [2, 1, 1, 0],
            BayerPattern::Grbg => [1, 0, 2, 1],
            BayerPattern::Gbrg => [1, 2, 0, 1],
        }
    }
    /// COLORID of the SER file format.
    pub fn ser_color_id(self) -> u32 {
        match self { BayerPattern::Rggb => 8, BayerPattern::Grbg => 9, BayerPattern::Gbrg => 10, BayerPattern::Bggr => 11 }
    }
    pub fn name(self) -> &'static str {
        match self { BayerPattern::Rggb => "RGGB", BayerPattern::Bggr => "BGGR", BayerPattern::Grbg => "GRBG", BayerPattern::Gbrg => "GBRG" }
    }
    /// The pattern after dropping `dx` columns and `dy` rows from the left/top (an odd crop swaps the layout).
    pub fn shifted(self, dx: u32, dy: u32) -> BayerPattern {
        let b = self.block();
        let at = |x: u32, y: u32| b[((y & 1) * 2 + (x & 1)) as usize];
        match [at(dx, dy), at(dx + 1, dy), at(dx, dy + 1), at(dx + 1, dy + 1)] {
            [0, 1, 1, 2] => BayerPattern::Rggb,
            [2, 1, 1, 0] => BayerPattern::Bggr,
            [1, 0, 2, 1] => BayerPattern::Grbg,
            _ => BayerPattern::Gbrg,
        }
    }
}

/// A single monochrome image. Colour/Bayer sensors are out of scope for now:
/// solar imaging is overwhelmingly done with mono cameras.
#[derive(Clone, Debug)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    pub data: Vec<u8>,
    pub captured_at: SystemTime,
    /// Set for raw frames of a one-shot-colour sensor: `data` is then the undemosaiced mosaic.
    pub bayer: Option<BayerPattern>,
}

impl Frame {
    pub fn new(width: u32, height: u32, format: PixelFormat, data: Vec<u8>) -> Frame {
        assert_eq!(data.len(), width as usize * height as usize * format.bytes_per_pixel());
        Frame { width, height, format, data, captured_at: SystemTime::now(), bayer: None }
    }

    #[inline]
    pub fn pixel(&self, x: u32, y: u32) -> u16 {
        let i = y as usize * self.width as usize + x as usize;
        match self.format {
            PixelFormat::Mono8 => self.data[i] as u16,
            PixelFormat::Mono16 => u16::from_le_bytes([self.data[2 * i], self.data[2 * i + 1]]),
        }
    }

    /// Mean pixel value as a fraction of full scale (0..1). Subsamples for speed.
    pub fn mean_fraction(&self) -> f64 {
        let step = 4usize;
        let (mut sum, mut n) = (0f64, 0u64);
        for y in (0..self.height).step_by(step) {
            for x in (0..self.width).step_by(step) {
                sum += self.pixel(x, y) as f64;
                n += 1;
            }
        }
        if n == 0 { 0.0 } else { sum / n as f64 / self.format.full_scale() }
    }

    /// Fraction of (subsampled) pixels at full scale.
    pub fn saturated_fraction(&self) -> f64 {
        let max = self.format.full_scale() as u16;
        let (mut sat, mut n) = (0u64, 0u64);
        for y in (0..self.height).step_by(4) {
            for x in (0..self.width).step_by(4) {
                n += 1;
                if self.pixel(x, y) >= max { sat += 1; }
            }
        }
        if n == 0 { 0.0 } else { sat as f64 / n as f64 }
    }

    /// Normalised histogram (`bins` buckets over full scale; values sum to 1). Subsamples for speed.
    pub fn histogram(&self, bins: usize) -> Vec<f32> {
        let mut h = vec![0u32; bins];
        let full = self.format.full_scale() + 1.0;
        let mut n = 0u32;
        for y in (0..self.height).step_by(4) {
            for x in (0..self.width).step_by(4) {
                let b = ((self.pixel(x, y) as f64 / full) * bins as f64) as usize;
                h[b.min(bins - 1)] += 1;
                n += 1;
            }
        }
        h.into_iter().map(|c| c as f32 / n.max(1) as f32).collect()
    }

    /// 8-bit copy (16-bit data is shifted down).
    pub fn to_mono8(&self) -> Vec<u8> {
        match self.format {
            PixelFormat::Mono8 => self.data.clone(),
            PixelFormat::Mono16 => self.data.chunks_exact(2).map(|c| c[1]).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn histogram_sums_to_one() {
        let f = Frame::new(8, 8, PixelFormat::Mono8, (0..64).map(|i| (i * 4) as u8).collect());
        let h = f.histogram(16);
        assert!((h.iter().sum::<f32>() - 1.0).abs() < 1e-5);
        assert!(h[0] > 0.0);
    }

    #[test]
    fn mono16_pixel_and_mean() {
        let f = Frame::new(2, 1, PixelFormat::Mono16, vec![0xff, 0xff, 0x00, 0x00]);
        assert_eq!(f.pixel(0, 0), 65535);
        assert_eq!(f.pixel(1, 0), 0);
        assert_eq!(f.to_mono8(), vec![0xff, 0x00]);
    }

    #[test]
    fn bayer_layouts_and_crop_shifts() {
        assert_eq!(BayerPattern::Rggb.block(), [0, 1, 1, 2]);
        assert_eq!(BayerPattern::Rggb.shifted(1, 0), BayerPattern::Grbg);
        assert_eq!(BayerPattern::Rggb.shifted(0, 1), BayerPattern::Gbrg);
        assert_eq!(BayerPattern::Rggb.shifted(1, 1), BayerPattern::Bggr);
        assert_eq!(BayerPattern::Rggb.shifted(2, 2), BayerPattern::Rggb);
        assert_eq!(BayerPattern::Bggr.ser_color_id(), 11);
    }
}
