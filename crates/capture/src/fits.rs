//! Minimal FITS writer: one primary HDU per frame, 8-bit unsigned or 16-bit (BZERO = 32768) data.
//! FITS stores the bottom row first, so rows are written bottom-to-top to match what PNG and the live view show.

use rustflare_core::{Error, Frame, PixelFormat, Result};
use std::io::Write;
use std::path::Path;
use std::time::SystemTime;

/// Optional header values describing how the frame was taken.
#[derive(Clone, Debug, Default)]
pub struct FitsMeta {
    pub instrument: Option<String>,
    pub exposure_us: Option<f64>,
    pub gain: Option<f64>,
    pub object: Option<String>,
    /// Extra free-text history cards (e.g. "flat applied: no").
    pub comments: Vec<String>,
}

fn card(key: &str, value: &str, comment: &str) -> String {
    // numbers and logicals are right-justified to column 30; strings start at column 11
    let mut c = if value.starts_with('\'') { format!("{key:<8}= {value:<20}") } else { format!("{key:<8}= {value:>20}") };
    if !comment.is_empty() {
        c.push_str(" / ");
        c.push_str(comment);
    }
    c.truncate(80);
    format!("{c:<80}")
}

fn text_card(key: &str, text: &str) -> String {
    let t: String = text.chars().filter(|c| c.is_ascii() && *c != '\'').take(60).collect();
    card(key, &format!("'{t:<8}'"), "")
}

/// ISO 8601 UTC timestamp (to milliseconds) for a `SystemTime`, without any date library.
pub fn iso8601(t: SystemTime) -> String {
    let d = t.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs() as i64;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}", rem / 3600, rem % 3600 / 60, rem % 60, d.subsec_millis())
}

/// Builds the whole file in memory (header padded to 2880 bytes, data padded likewise).
pub fn encode(frame: &Frame, meta: &FitsMeta) -> Vec<u8> {
    let (w, h) = (frame.width as usize, frame.height as usize);
    let sixteen = frame.format == PixelFormat::Mono16;
    let mut cards = vec![
        card("SIMPLE", "T", "conforms to FITS standard"),
        card("BITPIX", if sixteen { "16" } else { "8" }, ""),
        card("NAXIS", "2", ""),
        card("NAXIS1", &w.to_string(), "width"),
        card("NAXIS2", &h.to_string(), "height"),
    ];
    if sixteen {
        cards.push(card("BZERO", "32768", "unsigned 16-bit offset"));
        cards.push(card("BSCALE", "1", ""));
    }
    cards.push(card("DATE-OBS", &format!("'{}'", iso8601(frame.captured_at)), "UTC, start of frame readout (approx.)"));
    cards.push(text_card("SWCREATE", "Rustflare"));
    if let Some(i) = &meta.instrument {
        cards.push(text_card("INSTRUME", i));
    }
    if let Some(o) = &meta.object {
        cards.push(text_card("OBJECT", o));
    }
    if let Some(e) = meta.exposure_us {
        cards.push(card("EXPTIME", &format!("{:.6}", e / 1e6), "exposure, seconds"));
    }
    if let Some(g) = meta.gain {
        cards.push(card("GAIN", &format!("{g}"), "camera gain setting"));
    }
    for c in &meta.comments {
        let t: String = c.chars().filter(|c| c.is_ascii()).take(72).collect();
        cards.push(format!("{:<80}", format!("COMMENT {t}")));
    }
    cards.push(format!("{:<80}", "END"));
    let mut out: Vec<u8> = cards.concat().into_bytes();
    while out.len() % 2880 != 0 {
        out.push(b' ');
    }
    let bpp = frame.format.bytes_per_pixel();
    for y in (0..h).rev() {
        let row = &frame.data[y * w * bpp..(y + 1) * w * bpp];
        if sixteen {
            for px in row.chunks_exact(2) {
                let v = u16::from_le_bytes([px[0], px[1]]) as i32 - 32768;
                out.extend_from_slice(&(v as i16).to_be_bytes());
            }
        } else {
            out.extend_from_slice(row);
        }
    }
    while out.len() % 2880 != 0 {
        out.push(0);
    }
    out
}

pub fn write_fits(path: &Path, frame: &Frame, meta: &FitsMeta) -> Result<()> {
    let bytes = encode(frame, meta);
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    f.write_all(&bytes).map_err(Error::Io)?;
    f.flush().map_err(Error::Io)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_and_data_follow_the_standard() {
        // 2x2, bottom row (y=1) must come first; 16-bit values offset by 32768, big endian
        let f = Frame::new(2, 2, PixelFormat::Mono16, [1u16, 2, 65535, 32768].iter().flat_map(|v| v.to_le_bytes()).collect());
        let b = encode(&f, &FitsMeta { instrument: Some("Cam".into()), exposure_us: Some(2500.0), gain: Some(71.0), ..Default::default() });
        assert_eq!(b.len() % 2880, 0);
        let head = std::str::from_utf8(&b[..2880]).unwrap();
        assert!(head.starts_with("SIMPLE  =                    T"));
        assert!(head.contains("BITPIX  =                   16") && head.contains("BZERO   =                32768"));
        assert!(head.contains("NAXIS1  =                    2") && head.contains("EXPTIME ="));
        assert!(head.contains("INSTRUME= 'Cam     '") && head.contains("END"));
        for i in 0..head.len() / 80 {
            assert_eq!(head[i * 80..(i + 1) * 80].len(), 80);
        }
        let d = &b[2880..2880 + 8];
        let vals: Vec<i16> = d.chunks(2).map(|c| i16::from_be_bytes([c[0], c[1]])).collect();
        // bottom row first: [65535, 32768] then [1, 2], each minus 32768
        assert_eq!(vals, vec![32767, 0, -32767, -32766]);
    }

    #[test]
    fn iso_time_is_right() {
        let t = SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(1_795_296_038_123);
        assert_eq!(iso8601(t), "2026-11-21T21:20:38.123");
        assert_eq!(iso8601(SystemTime::UNIX_EPOCH), "1970-01-01T00:00:00.000");
    }

    #[test]
    fn eight_bit_is_unsigned_bitpix_8() {
        let f = Frame::new(2, 1, PixelFormat::Mono8, vec![7, 9]);
        let b = encode(&f, &FitsMeta::default());
        assert!(std::str::from_utf8(&b[..2880]).unwrap().contains("BITPIX  =                    8"));
        assert_eq!(&b[2880..2882], &[7, 9]);
    }
}
