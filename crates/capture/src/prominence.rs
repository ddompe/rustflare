//! Prominence pair: the disc is exposed for the disc, a second, much longer exposure shows the prominences (the disc
//! is blown out in it). The composite takes the disc from the short frame, scaled to the long exposure's units, and
//! everything outside the limb from the long frame.

use rustflare_core::{Error, Frame, PixelFormat, Result};

/// `factor` is long exposure / short exposure. Both frames must be Mono16 (or Mono8) of the same size.
pub fn composite(short: &Frame, long: &Frame, cx: f64, cy: f64, r: f64, factor: f64) -> Result<Frame> {
    if (short.width, short.height, short.format) != (long.width, long.height, long.format) {
        return Err(Error::Invalid("the two frames differ in size or format".into()));
    }
    let full = long.format.full_scale();
    let (w, h) = (short.width as usize, short.height as usize);
    let mut out: Vec<u16> = Vec::with_capacity(w * h);
    for y in 0..h {
        let dy = y as f64 - cy;
        for x in 0..w {
            let dx = x as f64 - cx;
            let inside = dx * dx + dy * dy <= r * r;
            let v = if inside { (short.pixel(x as u32, y as u32) as f64 * factor).min(full) } else { long.pixel(x as u32, y as u32) as f64 };
            // always written as 16 bit; 8-bit sources are scaled so the PNG uses the whole range
            out.push(if long.format == PixelFormat::Mono8 { (v * 257.0) as u16 } else { v as u16 });
        }
    }
    Ok(Frame::new(short.width, short.height, PixelFormat::Mono16, out.iter().flat_map(|v| v.to_le_bytes()).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f16(w: u32, h: u32, f: impl Fn(u32, u32) -> u16) -> Frame {
        let mut d = Vec::new();
        for y in 0..h { for x in 0..w { d.extend_from_slice(&f(x, y).to_le_bytes()); } }
        Frame::new(w, h, PixelFormat::Mono16, d)
    }

    #[test]
    fn disc_from_the_short_frame_prominence_from_the_long_one() {
        // disc radius 10 at (20,20): short frame disc 5000, sky 10; long (x8) frame: disc blown out, prominence 800 just outside
        let inside = |x: u32, y: u32| (x as i32 - 20).pow(2) + (y as i32 - 20).pow(2) <= 100;
        let short = f16(40, 40, |x, y| if inside(x, y) { 5000 } else { 10 });
        let long = f16(40, 40, |x, y| if inside(x, y) { 65535 } else if x == 31 && y == 20 { 800 } else { 80 });
        let c = composite(&short, &long, 20.0, 20.0, 10.0, 8.0).unwrap();
        assert_eq!(c.pixel(20, 20), 40000, "disc: short x 8");
        assert_eq!(c.pixel(31, 20), 800, "prominence from the long frame");
        assert_eq!(c.pixel(2, 2), 80);
        assert!(composite(&short, &f16(10, 10, |_, _| 0), 0.0, 0.0, 1.0, 2.0).is_err());
    }
}
