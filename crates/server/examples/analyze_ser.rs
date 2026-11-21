//! Runs the real Sun-location code over every frame of a SER file and reports how stable it is.
//!
//!   cargo run --release -p rustflare-server --example analyze_ser -- file.ser [expected_radius_px]
//!
//! The scatter of the located centre from frame to frame is, to first order, the seeing.

use rustflare_core::{Frame, PixelFormat};
use rustflare_tracking::locate_sun;
use std::io::{Read, Seek, SeekFrom};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("usage: analyze_ser <file.ser> [expected_radius_px]");
    let expected: Option<f64> = args.get(2).and_then(|s| s.parse().ok());
    let mut f = std::fs::File::open(path).expect("open");
    let mut h = [0u8; 178];
    f.read_exact(&mut h).unwrap();
    let u = |o: usize| u32::from_le_bytes(h[o..o + 4].try_into().unwrap());
    let (w, hh, depth, n) = (u(26), u(30), u(34), u(38));
    let fmt = if depth == 16 { PixelFormat::Mono16 } else { PixelFormat::Mono8 };
    let bytes = (w * hh) as usize * fmt.bytes_per_pixel();
    println!("{path}: {w}x{hh} {depth}-bit, {n} frames, expected radius {expected:?}");

    let (mut xs, mut ys, mut rs, mut partial) = (vec![], vec![], vec![], 0);
    for i in 0..n as u64 {
        f.seek(SeekFrom::Start(178 + i * bytes as u64)).unwrap();
        let mut data = vec![0u8; bytes];
        f.read_exact(&mut data).unwrap();
        let frame = Frame::new(w, hh, fmt, data);
        if let Some(s) = locate_sun(&frame, expected) {
            xs.push(s.pos.x);
            ys.push(s.pos.y);
            rs.push(s.radius);
            partial += s.partial as u32;
        }
    }
    let stats = |v: &[f64]| {
        let m = v.iter().sum::<f64>() / v.len().max(1) as f64;
        let sd = (v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / v.len().max(1) as f64).sqrt();
        let (lo, hi) = v.iter().fold((f64::MAX, f64::MIN), |(l, h), x| (l.min(*x), h.max(*x)));
        (m, sd, lo, hi)
    };
    println!("located in {}/{} frames ({} partial)", xs.len(), n, partial);
    for (name, v) in [("centre x", &xs), ("centre y", &ys), ("radius", &rs)] {
        let (m, sd, lo, hi) = stats(v);
        println!("  {name:9} mean {m:8.1}  std {sd:6.2}  range [{lo:.1}, {hi:.1}]");
    }
    // frame-to-frame step (what a guider would see as jitter), and slow drift over the burst
    let steps: Vec<f64> = xs.windows(2).zip(ys.windows(2)).map(|(a, b)| ((a[1] - a[0]).powi(2) + (b[1] - b[0]).powi(2)).sqrt()).collect();
    let (ms, _, _, mx) = stats(&steps);
    println!("  frame-to-frame centre step: mean {ms:.2} px, max {mx:.2} px");
    if xs.len() > 20 {
        let k = xs.len() / 4;
        let avg = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
        println!("  drift first quarter -> last quarter: dx {:+.2} px, dy {:+.2} px", avg(&xs[xs.len() - k..]) - avg(&xs[..k]), avg(&ys[ys.len() - k..]) - avg(&ys[..k]));
    }
}
