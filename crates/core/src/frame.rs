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

/// What the live view and the exposure helpers need from every frame: see `Frame::stats`.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameStats {
    pub mean: f64,
    pub saturated: f64,
    pub histogram: Vec<f32>,
    /// One-shot-colour frames only: normalised R, G and B histograms from the raw mosaic (see `Frame::channel_stats`).
    pub channels: Option<ChannelStats>,
}

/// Per-channel statistics of a Bayer mosaic: each photosite counts in its own channel (0 = R, 1 = G, 2 = B).
#[derive(Clone, Debug, PartialEq)]
pub struct ChannelStats {
    pub histogram: [Vec<f32>; 3],
    /// Fraction of each channel's sampled photosites at full scale.
    pub saturated: [f64; 3],
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

    /// Mean, saturated fraction and a normalised histogram in ONE pass over every 4th pixel of every 4th row (the three separate
    /// functions below read the same pixels three times). Same numbers as them.
    pub fn stats(&self, bins: usize) -> FrameStats {
        let bins = bins.max(1);
        let (w, h) = (self.width as usize, self.height as usize);
        let full = self.format.full_scale();
        let max = full as u16;
        let scale = bins as f64 / (full + 1.0);
        let mut hist = vec![0u32; bins];
        let (mut sum, mut sat, mut n) = (0u64, 0u64, 0u64);
        let mut tally = |v: u16| {
            sum += v as u64;
            sat += (v >= max) as u64;
            hist[((v as f64 * scale) as usize).min(bins - 1)] += 1;
            n += 1;
        };
        match self.format {
            PixelFormat::Mono8 => {
                for y in (0..h).step_by(4) {
                    for &b in self.data[y * w..(y + 1) * w].iter().step_by(4) {
                        tally(b as u16);
                    }
                }
            }
            PixelFormat::Mono16 => {
                for y in (0..h).step_by(4) {
                    for c in self.data[2 * y * w..2 * (y + 1) * w].chunks_exact(2).step_by(4) {
                        tally(u16::from_le_bytes([c[0], c[1]]));
                    }
                }
            }
        }
        let nn = n.max(1) as f64;
        FrameStats { mean: if n == 0 { 0.0 } else { sum as f64 / nn / full }, saturated: if n == 0 { 0.0 } else { sat as f64 / nn }, histogram: hist.into_iter().map(|c| c as f32 / nn as f32).collect(), channels: self.channel_stats(bins) }
    }

    /// R, G and B histograms of a Bayer frame, None for mono. Samples the 2x2 blocks at every 8th column and row (the same 1/16 of
    /// the pixels as `stats`, but all four photosites of a block, so every channel is seen: a stride of 4 would only ever land on
    /// one photosite). Green has two sites per block and is normalised by its own count.
    pub fn channel_stats(&self, bins: usize) -> Option<ChannelStats> {
        let pat = self.bayer?;
        let bins = bins.max(1);
        let (w, h) = (self.width as usize, self.height as usize);
        let full = self.format.full_scale();
        let max = full as u16;
        let scale = bins as f64 / (full + 1.0);
        let block = pat.block();
        let mut hist = [vec![0u32; bins], vec![0u32; bins], vec![0u32; bins]];
        let (mut sat, mut n) = ([0u64; 3], [0u64; 3]);
        for by in (0..h.saturating_sub(1)).step_by(8) {
            for bx in (0..w.saturating_sub(1)).step_by(8) {
                for k in 0..4 {
                    let (x, y) = (bx + (k & 1), by + (k >> 1));
                    let v = self.pixel(x as u32, y as u32);
                    let c = block[k];
                    hist[c][((v as f64 * scale) as usize).min(bins - 1)] += 1;
                    sat[c] += (v >= max) as u64;
                    n[c] += 1;
                }
            }
        }
        let norm = |c: usize| hist[c].iter().map(|&v| v as f32 / n[c].max(1) as f32).collect::<Vec<f32>>();
        Some(ChannelStats { histogram: [norm(0), norm(1), norm(2)], saturated: [0, 1, 2].map(|c| sat[c] as f64 / n[c].max(1) as f64) })
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

#[cfg(test)]
mod stats_tests {
    use super::*;

    #[test]
    fn the_one_pass_statistics_equal_the_three_separate_ones() {
        for (w, h, fmt) in [(37u32, 29u32, PixelFormat::Mono16), (64, 48, PixelFormat::Mono8), (1944, 1472, PixelFormat::Mono16)] {
            let bpp = fmt.bytes_per_pixel();
            let mut data = vec![0u8; w as usize * h as usize * bpp];
            let mut s = 12345u32;
            for px in data.chunks_exact_mut(bpp) {
                s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                let v = ((s >> 8) % if bpp == 2 { 65536 } else { 256 }) as u32;
                if bpp == 2 { px.copy_from_slice(&(v as u16).to_le_bytes()) } else { px[0] = v as u8 }
            }
            let f = Frame::new(w, h, fmt, data);
            let st = f.stats(64);
            assert!((st.mean - f.mean_fraction()).abs() < 1e-12, "{w}x{h}");
            assert!((st.saturated - f.saturated_fraction()).abs() < 1e-12);
            assert_eq!(st.histogram, f.histogram(64));
        }
    }

    #[test]
    fn channel_histograms_follow_the_pattern_and_mono_has_none() {
        // 32x32 mosaic: every R site 60000, G 20000, B 65535 (clipped), for each pattern
        for pat in [BayerPattern::Rggb, BayerPattern::Bggr, BayerPattern::Grbg, BayerPattern::Gbrg] {
            let block = pat.block();
            let mut data = vec![0u8; 32 * 32 * 2];
            for y in 0..32usize {
                for x in 0..32usize {
                    let v: u16 = match block[(y & 1) * 2 + (x & 1)] { 0 => 60000, 1 => 20000, _ => 65535 };
                    data[2 * (y * 32 + x)..2 * (y * 32 + x) + 2].copy_from_slice(&v.to_le_bytes());
                }
            }
            let mut f = Frame::new(32, 32, PixelFormat::Mono16, data);
            assert!(f.stats(64).channels.is_none());
            f.bayer = Some(pat);
            let c = f.stats(64).channels.expect("colour frame has channel stats");
            for (ch, bin) in [(0usize, 60000 * 64 / 65536), (1, 20000 * 64 / 65536), (2, 63)] {
                assert!((c.histogram[ch].iter().sum::<f32>() - 1.0).abs() < 1e-5, "{pat:?} channel {ch} sums to one");
                assert!((c.histogram[ch][bin] - 1.0).abs() < 1e-5, "{pat:?} channel {ch} sits in bin {bin}");
            }
            assert_eq!(c.saturated, [0.0, 0.0, 1.0]);
        }
    }
}
