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

/// Reads a FITS still written by `write` (and similar): 16-bit (with BZERO) or 32-bit float, two axes. Rows come back top-down
/// (the file has them bottom-up when ROWORDER says so, as ours do).
pub fn read(path: &Path) -> Result<(usize, usize, Vec<f32>), String> {
    let b = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let end = b.windows(80).step_by(80).position(|c| c.starts_with(b"END     ")).ok_or("not a FITS file")?;
    let head = String::from_utf8_lossy(&b[..(end + 1) * 80]).to_string();
    let key = |k: &str| -> Option<String> {
        head.as_bytes().chunks(80).find_map(|c| {
            let line = std::str::from_utf8(c).ok()?;
            (line.starts_with(&format!("{k:<8}="))).then(|| line[10..].split('/').next().unwrap_or("").trim().trim_matches('\'').trim().to_string())
        })
    };
    let num = |k: &str| key(k).and_then(|v| v.parse::<f64>().ok());
    let (bitpix, w, h) = (num("BITPIX").ok_or("no BITPIX")? as i64, num("NAXIS1").ok_or("no NAXIS1")? as usize, num("NAXIS2").ok_or("no NAXIS2")? as usize);
    let bzero = num("BZERO").unwrap_or(0.0) as f32;
    let data = &b[(end * 80 / 2880 + 1) * 2880..];
    let flip = key("ROWORDER").map_or(false, |v| v.eq_ignore_ascii_case("BOTTOM-UP"));
    let n = w * h;
    let mut v: Vec<f32> = match bitpix {
        16 if data.len() >= n * 2 => data[..n * 2].chunks_exact(2).map(|c| i16::from_be_bytes([c[0], c[1]]) as f32 + bzero).collect(),
        -32 if data.len() >= n * 4 => data[..n * 4].chunks_exact(4).map(|c| f32::from_be_bytes([c[0], c[1], c[2], c[3]])).collect(),
        _ => return Err(format!("unsupported FITS data (BITPIX {bitpix}) or a truncated file")),
    };
    if flip {
        let rows: Vec<Vec<f32>> = v.chunks(w).map(|r| r.to_vec()).collect();
        v = rows.into_iter().rev().flatten().collect();
    }
    Ok((w, h, v))
}

/// An 8-bit preview no wider than `max_w`: box-averaged down, then stretched between the 1st and 99.7th percentile with a gamma of
/// 0.7. For display only. Returns (width, height, pixels top-down).
pub fn preview(path: &Path, max_w: usize) -> Result<(usize, usize, Vec<u8>), String> {
    let (w, h, v) = read(path)?;
    let f = (w + max_w - 1) / max_w.max(1);
    let f = f.max(1);
    let (ow, oh) = (w / f, h / f);
    let mut small = vec![0f32; ow * oh];
    for y in 0..oh {
        for x in 0..ow {
            let mut s = 0.0;
            for dy in 0..f {
                for dx in 0..f {
                    s += v[(y * f + dy) * w + x * f + dx];
                }
            }
            small[y * ow + x] = s / (f * f) as f32;
        }
    }
    let mut sorted: Vec<f32> = small.iter().step_by(4).copied().collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let at = |p: f64| sorted[((sorted.len() - 1) as f64 * p) as usize];
    let (lo, hi) = (at(0.01), at(0.997));
    let span = (hi - lo).max(1e-6);
    Ok((ow, oh, small.iter().map(|&p| (((p - lo) / span).clamp(0.0, 1.0).powf(0.7) * 255.0) as u8).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_written_reads_back_upright_in_both_formats() {
        let dir = std::env::temp_dir().join(format!("rf-fits-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (w, h) = (12, 8);
        let data: Vec<f32> = (0..w * h).map(|i| (i * 100) as f32).collect(); // row 0 (top) is the darkest
        for float in [true, false] {
            let p = dir.join(if float { "f.fits" } else { "u.fits" });
            write(&p, w, h, &data, float, &[("EXPTIME".into(), "0.5".into(), "".into())]).unwrap();
            let (rw, rh, back) = read(&p).unwrap();
            assert_eq!((rw, rh), (w, h));
            assert_eq!(back, data, "float = {float}");
        }
        let (pw, ph, px) = preview(&dir.join("f.fits"), 6).unwrap();
        assert_eq!((pw, ph), (6, 4));
        assert!(px[0] < px[px.len() - 1], "the top row of the picture is the first row of the data");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
