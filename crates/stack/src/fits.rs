//! FITS output for a stacked still: unsigned 16-bit (BZERO = 32768) or 32-bit float, rows bottom-up like the recorder's FITS.

use std::io::Write;
use std::path::Path;

fn card(key: &str, value: &str, comment: &str) -> String {
    let mut s = format!("{key:<8}= {value:>20}");
    if !comment.is_empty() {
        s.push_str(&format!(" / {comment}"));
    }
    s.truncate(80);
    format!("{s:<80}")
}

fn text(v: &str) -> String { format!("'{}'", v.replace('\'', "''")) }

/// Header entries beyond the structural ones: (key, already formatted value, comment).
pub type Extra = (String, String, String);

pub fn write(path: &Path, w: usize, h: usize, data: &[f32], float: bool, extra: &[Extra]) -> std::io::Result<()> {
    let mut cards = vec![
        card("SIMPLE", "T", "conforms to FITS"),
        card("BITPIX", if float { "-32" } else { "16" }, ""),
        card("NAXIS", "2", ""),
        card("NAXIS1", &w.to_string(), ""),
        card("NAXIS2", &h.to_string(), ""),
    ];
    if !float {
        cards.push(card("BZERO", "32768", "unsigned 16-bit offset"));
        cards.push(card("BSCALE", "1", ""));
    }
    cards.push(card("ROWORDER", &text("BOTTOM-UP"), "first row is the bottom of the image"));
    cards.push(card("SWCREATE", &text("rustflare-stack"), ""));
    for (k, v, c) in extra {
        cards.push(card(k, v, c));
    }
    cards.push(format!("{:<80}", "END"));
    let mut head: Vec<u8> = cards.concat().into_bytes();
    while head.len() % 2880 != 0 {
        head.push(b' ');
    }
    let mut body = Vec::with_capacity(w * h * if float { 4 } else { 2 });
    for y in (0..h).rev() {
        for &v in &data[y * w..(y + 1) * w] {
            if float {
                body.extend_from_slice(&v.to_be_bytes());
            } else {
                let u = v.round().clamp(0.0, 65535.0) as u16;
                body.extend_from_slice(&(u ^ 0x8000).to_be_bytes());
            }
        }
    }
    while body.len() % 2880 != 0 {
        body.push(0);
    }
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    f.write_all(&head)?;
    f.write_all(&body)?;
    f.flush()
}

pub fn quoted(v: &str) -> String { text(v) }
