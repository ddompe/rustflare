use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use rustflare_core::{Error, Frame, PixelFormat, Result};

/// Writes a grayscale PNG (8 or 16 bit). 16-bit PNG samples are big-endian, so swap.
pub fn write_png(path: &Path, frame: &Frame) -> Result<()> {
    let file = BufWriter::new(File::create(path)?);
    let mut enc = png::Encoder::new(file, frame.width, frame.height);
    enc.set_color(png::ColorType::Grayscale);
    let data: Vec<u8> = match frame.format {
        PixelFormat::Mono8 => {
            enc.set_depth(png::BitDepth::Eight);
            frame.data.clone()
        }
        PixelFormat::Mono16 => {
            enc.set_depth(png::BitDepth::Sixteen);
            frame.data.chunks_exact(2).flat_map(|c| [c[1], c[0]]).collect()
        }
    };
    let mut w = enc.write_header().map_err(|e| Error::Io(std::io::Error::other(e)))?;
    w.write_image_data(&data).map_err(|e| Error::Io(std::io::Error::other(e)))?;
    Ok(())
}
