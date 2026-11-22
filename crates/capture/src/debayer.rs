//! Demosaicing and white balance for one-shot-colour sensors. Recordings stay raw; this is for the colour preview and
//! for colour PNG stills.

use rustflare_core::{BayerPattern, Frame, PixelFormat};

/// An interleaved RGB image of 16-bit samples (8-bit sources are scaled to 16 bit).
pub struct Rgb16 {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u16>,
}

fn sample(f: &Frame, x: usize, y: usize) -> u32 {
    let i = y * f.width as usize + x;
    match f.format {
        PixelFormat::Mono8 => f.data[i] as u32 * 257,
        PixelFormat::Mono16 => u16::from_le_bytes([f.data[2 * i], f.data[2 * i + 1]]) as u32,
    }
}

/// Bilinear demosaic: at every photosite the missing channels are the mean of the neighbours (3x3) of that colour,
/// then each channel is multiplied by its white-balance gain.
pub fn bilinear(frame: &Frame, pattern: BayerPattern, gains: [f64; 3]) -> Rgb16 {
    let (w, h) = (frame.width as usize, frame.height as usize);
    let blk = pattern.block();
    let mut data = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 3];
            let mut n = [0u32; 3];
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                        continue;
                    }
                    let c = blk[((ny as usize & 1) << 1) | (nx as usize & 1)];
                    sum[c] += sample(frame, nx as usize, ny as usize);
                    n[c] += 1;
                }
            }
            for c in 0..3 {
                let v = if n[c] == 0 { 0.0 } else { sum[c] as f64 / n[c] as f64 };
                data.push((v * gains[c]).round().clamp(0.0, 65535.0) as u16);
            }
        }
    }
    Rgb16 { width: frame.width, height: frame.height, data }
}

/// White-balance gains (R, G = 1, B) that make the bright part of the picture neutral: the Sun is taken as white. Uses
/// the 2x2 blocks (sampled sparsely) whose green is at least half of the brightest sampled green.
pub fn auto_white_balance(frame: &Frame, pattern: BayerPattern) -> [f64; 3] {
    let (bw, bh) = (frame.width as usize / 2, frame.height as usize / 2);
    let step = (bw.max(bh) / 200).max(1);
    let blk = pattern.block();
    let mut blocks = vec![];
    let mut gmax = 0.0f64;
    let mut by = 0;
    while by < bh {
        let mut bx = 0;
        while bx < bw {
            let mut s = [0.0f64; 3];
            let mut n = [0.0f64; 3];
            for (k, c) in blk.iter().enumerate() {
                s[*c] += sample(frame, bx * 2 + (k & 1), by * 2 + (k >> 1)) as f64;
                n[*c] += 1.0;
            }
            let v = [s[0] / n[0], s[1] / n[1], s[2] / n[2]];
            gmax = gmax.max(v[1]);
            blocks.push(v);
            bx += step;
        }
        by += step;
    }
    let (mut r, mut g, mut b, mut cnt) = (0.0, 0.0, 0.0, 0.0);
    for v in blocks.iter().filter(|v| v[1] >= gmax * 0.5 && v[1] > 0.0 && v[0] < 65000.0 && v[2] < 65000.0) {
        r += v[0];
        g += v[1];
        b += v[2];
        cnt += 1.0;
    }
    if cnt == 0.0 || r <= 0.0 || b <= 0.0 {
        return [1.0, 1.0, 1.0];
    }
    let _ = cnt;
    [(g / r).clamp(0.25, 4.0), 1.0, (g / b).clamp(0.25, 4.0)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn warm_mosaic() -> Frame {
        // RGGB, a uniform patch with R 40000, G 30000, B 15000
        let (w, h) = (8usize, 8usize);
        let mut d = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let v: u16 = match (x & 1, y & 1) { (0, 0) => 40000, (1, 1) => 15000, _ => 30000 };
                d.extend_from_slice(&v.to_le_bytes());
            }
        }
        let mut f = Frame::new(w as u32, h as u32, PixelFormat::Mono16, d);
        f.bayer = Some(BayerPattern::Rggb);
        f
    }

    #[test]
    fn a_flat_patch_demosaics_to_its_colour_everywhere() {
        let f = warm_mosaic();
        let img = bilinear(&f, BayerPattern::Rggb, [1.0; 3]);
        for p in img.data.chunks_exact(3) {
            assert_eq!(p, &[40000, 30000, 15000]);
        }
    }

    #[test]
    fn auto_balance_makes_the_patch_grey() {
        let f = warm_mosaic();
        let g = auto_white_balance(&f, BayerPattern::Rggb);
        assert!((40000.0 * g[0] - 30000.0).abs() < 1.0 && (15000.0 * g[2] - 30000.0).abs() < 1.0, "{g:?}");
        let img = bilinear(&f, BayerPattern::Rggb, g);
        assert_eq!(&img.data[..3], &[30000, 30000, 30000]);
    }

    #[test]
    fn a_dark_frame_gets_unit_gains() {
        let mut f = warm_mosaic();
        f.data.iter_mut().for_each(|b| *b = 0);
        assert_eq!(auto_white_balance(&f, BayerPattern::Rggb), [1.0, 1.0, 1.0]);
    }
}
