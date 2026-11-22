use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use rustflare_core::{Error, Frame, PixelFormat, Result};

/// Reads a grayscale 8 or 16-bit PNG (the format `write_png` produces) back into a frame.
pub fn read_png(path: &Path) -> Result<Frame> {
    let dec = png::Decoder::new(std::io::BufReader::new(File::open(path)?));
    let mut r = dec.read_info().map_err(|e| Error::Io(std::io::Error::other(e)))?;
    let mut buf = vec![0; r.output_buffer_size()];
    let info = r.next_frame(&mut buf).map_err(|e| Error::Io(std::io::Error::other(e)))?;
    if info.color_type != png::ColorType::Grayscale {
        return Err(Error::Invalid("expected a grayscale PNG".into()));
    }
    buf.truncate(info.buffer_size());
    match info.bit_depth {
        png::BitDepth::Eight => Ok(Frame::new(info.width, info.height, PixelFormat::Mono8, buf)),
        png::BitDepth::Sixteen => Ok(Frame::new(info.width, info.height, PixelFormat::Mono16, buf.chunks_exact(2).flat_map(|c| [c[1], c[0]]).collect())),
        _ => Err(Error::Invalid("unsupported PNG bit depth".into())),
    }
}

/// Writes a 16-bit colour PNG (the debayered picture of a one-shot-colour frame, white balance applied).
pub fn write_png_rgb(path: &Path, img: &crate::debayer::Rgb16) -> Result<()> {
    let file = BufWriter::new(File::create(path)?);
    let mut enc = png::Encoder::new(file, img.width, img.height);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Sixteen);
    let data: Vec<u8> = img.data.iter().flat_map(|v| v.to_be_bytes()).collect();
    let mut w = enc.write_header().map_err(|e| Error::Io(std::io::Error::other(e)))?;
    w.write_image_data(&data).map_err(|e| Error::Io(std::io::Error::other(e)))?;
    Ok(())
}

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
