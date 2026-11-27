//! The fast limb extraction against the one before the speed-up (`reference`), on synthetic frames of the real sizes,
//! and a timing report (`cargo test -p rustflare-tracking --release -- --ignored --nocapture limb_timing`).
use super::*;
use std::time::Instant;

struct Scene { w: u32, h: u32, sixteen: bool, cx: f64, cy: f64, r: f64, level: f64, noise: f64, shadows: bool, cloud: bool, seed: u32 }

fn scene(s: &Scene) -> Frame {
    let mut st = s.seed.wrapping_mul(2654435761).wrapping_add(7);
    let mut rnd = move || { st = st.wrapping_mul(1664525).wrapping_add(1013904223); (st >> 8) as f64 / 16777216.0 - 0.5 };
    let bpp = if s.sixteen { 2 } else { 1 };
    let mut d = Vec::with_capacity((s.w * s.h) as usize * bpp);
    for y in 0..s.h {
        for x in 0..s.w {
            let (dx, dy) = (x as f64 - s.cx, y as f64 - s.cy);
            let rr = (dx * dx + dy * dy).sqrt();
            let mut v = 0.8 + s.noise * rnd();
            if rr < s.r {
                let mut sig = 200.0 * s.level * (1.0 - 0.3 * rr / s.r);
                if s.shadows {
                    if (x as f64 - s.cx * 0.6).abs() < s.r * 0.05 { sig *= 0.08; }
                    if (x as f64 - (s.cx * 0.9 + (y as f64 - s.cy * 0.3) * 0.12)).abs() < 3.0 { sig = 0.0; }
                }
                if s.cloud { sig *= 0.5 + 0.5 * ((x as f64 / 150.0).sin() * (y as f64 / 170.0).cos()).abs(); }
                v += sig;
            }
            let v = v.max(0.0);
            if s.sixteen { d.extend_from_slice(&((v * 256.0 + 0.5).min(65535.0) as u16).to_le_bytes()); } else { d.push((v + 0.5).min(255.0) as u8); }
        }
    }
    Frame::new(s.w, s.h, if s.sixteen { PixelFormat::Mono16 } else { PixelFormat::Mono8 }, d)
}

fn small(cx: f64, cy: f64, r: f64, level: f64, noise: f64, shadows: bool, cloud: bool, seed: u32, sixteen: bool) -> Scene {
    Scene { w: 640, h: 480, sixteen, cx, cy, r, level, noise, shadows, cloud, seed }
}

fn fit(pts: &[(f64, f64)], expected: Option<f64>) -> Option<(Vec2, f64)> { fit_limb_pts(pts, None, expected) }

#[test]
fn the_fast_limb_points_equal_the_old_ones_on_noise_shadows_partial_discs_and_dim_frames() {
    let scenes = [
        small(320.0, 240.0, 150.0, 1.0, 3.0, false, false, 1, false),
        small(320.0, 240.0, 150.0, 1.0, 3.0, true, false, 2, true),
        small(520.0, 300.0, 380.0, 1.0, 2.0, true, false, 3, true),   // partial disc
        small(600.0, 450.0, 300.0, 1.0, 2.0, false, true, 4, true),   // Sun low and right, outside the frame
        small(100.0, 400.0, 260.0, 0.3, 1.0, false, true, 5, true),   // cloud, dim
        small(320.0, 240.0, 150.0, 0.01, 0.1, true, false, 6, true),  // hundredfold dimmer
        small(320.0, 240.0, 150.0, 0.004, 0.05, false, false, 7, true),
        small(300.0, 200.0, 120.0, 1.0, 8.0, true, true, 8, false),
        small(-50.0, 100.0, 200.0, 1.0, 2.0, false, false, 9, true),
    ];
    for (i, s) in scenes.iter().enumerate() {
        let f = scene(s);
        for expected in [None, Some(s.r), Some(s.r * 1.1)] {
            let (new, old) = (limb_points(&f, expected), reference::limb_points(&f, expected));
            assert_eq!(new, old, "scene {i} expected {expected:?}");
            match (fit(&new, expected), fit(&old, expected)) {
                (Some((c1, r1)), Some((c2, r2))) => assert!((c1 - c2).len() < 0.1 && (r1 - r2).abs() < 0.1, "scene {i}"),
                (a, b) => assert_eq!(a.is_some(), b.is_some()),
            }
        }
    }
}

#[test]
#[ignore]
fn limb_timing() {
    let big = |sixteen, cx, cy, r, level, shadows, cloud| Scene { w: 3008, h: 3008, sixteen, cx, cy, r, level, noise: 2.0, shadows, cloud, seed: 11 };
    let cases = [
        ("3008 8-bit centred", big(false, 1504.0, 1504.0, 1100.0, 1.0, false, false)),
        ("3008 16-bit centred", big(true, 1504.0, 1504.0, 1100.0, 1.0, false, false)),
        ("3008 16-bit low/partial", big(true, 1700.0, 2900.0, 1300.0, 1.0, false, false)),
        ("3008 16-bit shadows", big(true, 1504.0, 1504.0, 1100.0, 1.0, true, false)),
        ("3008 16-bit dim cloud", big(true, 1504.0, 1504.0, 1100.0, 0.02, false, true)),
        ("1944x1472 16-bit", Scene { w: 1944, h: 1472, sixteen: true, cx: 900.0, cy: 1300.0, r: 800.0, level: 1.0, noise: 2.0, shadows: true, cloud: false, seed: 12 }),
    ];
    for (name, s) in &cases {
        let f = scene(s);
        let t = |g: &dyn Fn() -> usize| { let t0 = Instant::now(); let mut n = 0; for _ in 0..5 { n = g(); } (t0.elapsed().as_secs_f64() * 200.0, n) };
        let (ms_new, n) = t(&|| limb_points(&f, Some(s.r)).len());
        let (ms_old, _) = t(&|| reference::limb_points(&f, Some(s.r)).len());
        let (ms_gain, _) = t(&|| view_gain(&f).is_some() as usize);
        let (ms_fit, _) = t(&|| { let p = limb_points(&f, Some(s.r)); fit_limb_pts(&p, None, None).is_some() as usize });
        let (ms_thr, _) = t(&|| { let v = view_gain(&f).unwrap(); sun_threshold(&sample_grid(&f, v)).unwrap_or(0) as usize });
        let raw = { let v = view_gain(&f).unwrap(); let _ = v; reference::limb_points(&f, None) };
        let (ms_trim, _) = t(&|| trim_outliers(raw.clone(), Some(s.r)).len());
        println!("   stages: threshold {ms_thr:.2}  trim_outliers (on {} kept pts) {ms_trim:.2}", raw.len());
        println!("{name:28} points {n:5}  limb_points new {ms_new:7.2} ms  old {ms_old:7.2} ms  (view_gain {ms_gain:.2})  +fit {ms_fit:.2}");
    }
}
