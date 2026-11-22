//! The stacker against synthetic recordings with known shifts, blur and noise.

use rustflare_stack::align::{accumulate_shifted, Aligner};
use rustflare_stack::calib::Calibration;
use rustflare_stack::image::{bin, box_blur, valid_mask, Img};
use rustflare_stack::stack::{stack_ser, Options, Rank};

/// Small deterministic pseudo-random numbers (no dependency).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) as f32) / (1u64 << 31) as f32
    }
    fn gauss(&mut self) -> f32 { (0..6).map(|_| self.next()).sum::<f32>() - 3.0 }
}

const W: usize = 320;
const H: usize = 240;

/// A bright disc with chromospheric-looking texture (smoothed noise) and a limb.
fn scene(seed: u64) -> Vec<f32> {
    let mut r = Rng(seed);
    let noise: Vec<f32> = (0..W * H).map(|_| r.next() - 0.5).collect();
    let tex = box_blur(&box_blur(&noise, W, H, 2), W, H, 2);
    let mut v = vec![0f32; W * H];
    for y in 0..H {
        for x in 0..W {
            let d = (((x as f32 - 160.0).powi(2) + (y as f32 - 120.0).powi(2)) as f32).sqrt();
            let disc = 1.0 / (1.0 + ((d - 90.0) / 1.5).exp());
            v[y * W + x] = 20000.0 * disc * (1.0 + 6.0 * tex[y * W + x]) + 300.0;
        }
    }
    v
}

fn shifted(src: &[f32], dx: f64, dy: f64) -> Vec<f32> {
    // content displaced by (dx, dy): out(x) = src(x - d)
    let mut out = vec![0f32; W * H];
    accumulate_shifted(&mut out, src, W, H, -dx, -dy);
    out
}

fn blur(src: &[f32], r: usize) -> Vec<f32> { if r == 0 { src.to_vec() } else { box_blur(src, W, H, r) } }

#[test]
fn alignment_recovers_known_shifts() {
    let base = scene(7);
    let valid = valid_mask(&Img { w: W, h: H, d: base.clone() }, 2.0, 8);
    let al = Aligner::new(W, H, &valid, 0.7);
    let refp = al.prepare(&Img { w: W, h: H, d: base.clone() });
    for (dx, dy) in [(0.0, 0.0), (3.0, -2.0), (-4.4, 1.7), (1.25, 5.5), (-0.6, -0.4)] {
        let mut img = shifted(&base, dx, dy);
        let mut r = Rng(11);
        for v in &mut img {
            *v += 150.0 * r.gauss();
        }
        let (mx, my) = al.shift(&refp, &al.prepare(&Img { w: W, h: H, d: img }));
        assert!((mx - dx).abs() < 0.15 && (my - dy).abs() < 0.15, "shift ({dx}, {dy}) measured ({mx:.2}, {my:.2})");
    }
}

#[test]
fn aligning_with_the_measured_shift_puts_the_content_back() {
    let base = scene(3);
    let (dx, dy) = (2.5, -3.5);
    let img = shifted(&base, dx, dy);
    let valid = valid_mask(&Img { w: W, h: H, d: base.clone() }, 2.0, 8);
    let al = Aligner::new(W, H, &valid, 0.7);
    let (mx, my) = al.shift(&al.prepare(&Img { w: W, h: H, d: base.clone() }), &al.prepare(&Img { w: W, h: H, d: img.clone() }));
    let mut back = vec![0f32; W * H];
    accumulate_shifted(&mut back, &img, W, H, mx, my);
    let err = |a: &[f32]| (60..H - 60).flat_map(|y| (60..W - 60).map(move |x| (x, y))).map(|(x, y)| ((a[y * W + x] - base[y * W + x]) as f64).powi(2)).sum::<f64>();
    assert!(err(&back) < 0.05 * err(&img), "aligned error {} vs unaligned {}", err(&back), err(&img));
}

fn write_ser(path: &std::path::Path, frames: &[Vec<f32>]) {
    use rustflare_core::{Frame, PixelFormat};
    let mut w = rustflare_capture::ser::SerWriter::create(path, W as u32, H as u32, PixelFormat::Mono16, "test", "test").unwrap();
    for f in frames {
        let data: Vec<u8> = f.iter().flat_map(|&v| (v.clamp(0.0, 65535.0) as u16).to_le_bytes()).collect();
        w.write_frame(&Frame::new(W as u32, H as u32, PixelFormat::Mono16, data)).unwrap();
    }
    w.finalize().unwrap();
}

/// A synthetic burst: every frame is the scene, shifted by seeing jitter, blurred by a varying amount, with noise.
fn burst(n: usize) -> (Vec<Vec<f32>>, Vec<usize>) {
    let base = scene(5);
    let mut r = Rng(99);
    let (mut frames, mut blurs) = (Vec::new(), Vec::new());
    for i in 0..n {
        let b = if i % 4 == 0 { 0 } else if i % 4 == 1 { 1 } else { 2 };
        blurs.push(b);
        let mut f = blur(&shifted(&base, (3.0 * r.gauss() / 2.0) as f64, (3.0 * r.gauss() / 2.0) as f64), b);
        for v in &mut f {
            *v = (*v + 250.0 * r.gauss()).max(3.0);
        }
        frames.push(f);
    }
    (frames, blurs)
}

#[test]
fn the_best_frames_are_the_sharp_ones_and_stacking_beats_the_mean() {
    let dir = std::env::temp_dir().join(format!("rf-stack-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("burst.ser");
    let (frames, blurs) = burst(48);
    write_ser(&path, &frames);
    let cal = Calibration::default();
    let o = Options { keep: 0.25, threads: 2, ..Options::default() };
    let best = stack_ser(&path, &o, &cal).unwrap();
    // the kept frames are the unblurred ones
    let unblurred = best.report.kept_frames.iter().filter(|&&i| blurs[i] == 0).count();
    assert!(unblurred >= best.report.kept * 3 / 4, "only {unblurred} of {} kept frames are the sharp ones", best.report.kept);
    // the shifts were found (jitter of about 1.5 px rms per axis)
    assert!(best.report.shift_rms_px > 0.8 && best.report.shift_rms_px < 4.0, "shift rms {}", best.report.shift_rms_px);
    // better than averaging everything, and better than not aligning
    let all = stack_ser(&path, &Options { keep: 1.0, ..o.clone() }, &cal).unwrap();
    let unaligned = stack_ser(&path, &Options { align: false, ..o.clone() }, &cal).unwrap();
    let worst = stack_ser(&path, &Options { rank: Rank::Worst, ..o.clone() }, &cal).unwrap();
    assert!(best.report.signal > all.report.signal * 1.15, "best 25% {:.6} vs all {:.6}", best.report.signal, all.report.signal);
    assert!(best.report.signal > unaligned.report.signal * 1.3, "aligned {:.6} vs unaligned {:.6}", best.report.signal, unaligned.report.signal);
    assert!(best.report.signal > worst.report.signal * 1.5, "best {:.6} vs worst {:.6}", best.report.signal, worst.report.signal);
    // and the stack is a plausible image: same size, brightness of the scene
    assert_eq!((best.width, best.height), (W, H));
    let c = best.data[120 * W + 160];
    assert!(c > 1000.0 && c < 60000.0);
    let _ = bin(&best.data, W, H, 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn split_files_read_as_one_recording_and_cut_files_report_what_they_hold() {
    use rustflare_stack::ser::Ser;
    let dir = std::env::temp_dir().join(format!("rf-stack-ser-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (frames, _) = burst(6);
    write_ser(&dir.join("take.ser"), &frames[..4]);
    write_ser(&dir.join("take_part2.ser"), &frames[4..]);
    let mut s = Ser::open(&dir.join("take.ser")).unwrap();
    assert_eq!((s.width, s.height, s.frames()), (W, H, 6));
    let f5 = s.read_frame(5).unwrap();
    assert_eq!(f5[1000], frames[5][1000].clamp(0.0, 65535.0) as u16); // the sixth frame is the second of the second part
    assert!(s.read_frame(6).is_err());
    assert!(s.timestamp(0).is_some());
    // a file cut in the middle of its last frame (a pulled drive): the header may still claim every frame
    let cut = dir.join("cut.ser");
    write_ser(&cut, &frames);
    let len = std::fs::metadata(&cut).unwrap().len();
    std::fs::OpenOptions::new().write(true).open(&cut).unwrap().set_len(len - 8 * 6 - (W * H * 2 / 2) as u64).unwrap();
    let mut c = Ser::open(&cut).unwrap();
    assert_eq!(c.frames(), 5);
    assert!(c.read_frame(4).is_ok());
    assert!(c.timestamp(0).is_none()); // the trailer is gone
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn masters_are_built_from_the_raw_recordings() {
    let dir = std::env::temp_dir().join(format!("rf-stack-cal-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut r = Rng(5);
    let dark: Vec<Vec<f32>> = (0..8).map(|_| (0..W * H).map(|_| 200.0 + 4.0 * r.gauss()).collect()).collect();
    // a flat with vignetting, on top of the same dark level
    let vig = |x: usize, y: usize| 1.0 - 0.4 * ((((x as f32 - 160.0).powi(2) + (y as f32 - 120.0).powi(2)).sqrt()) / 200.0).powi(2);
    let flat: Vec<Vec<f32>> = (0..8).map(|_| (0..W * H).map(|i| 200.0 + 30000.0 * vig(i % W, i / W) + 20.0 * r.gauss()).collect()).collect();
    write_ser(&dir.join("dark.ser"), &dark);
    write_ser(&dir.join("flat.ser"), &flat);
    let cal = Calibration::load(Some(&dir.join("dark.ser")), Some(&dir.join("flat.ser"))).unwrap();
    let d = cal.dark.as_ref().unwrap();
    assert!((d.iter().sum::<f32>() / d.len() as f32 - 200.0).abs() < 1.0); // the test frames are truncated to integers
    let f = cal.flat.as_ref().unwrap();
    assert!((f.iter().sum::<f32>() / f.len() as f32 - 1.0).abs() < 0.01, "the flat is normalised to a mean of 1");
    assert!(f[120 * W + 160] > 1.1 && f[5 * W + 5] < 0.8, "vignetting is in the flat: centre {} corner {}", f[120 * W + 160], f[5 * W + 5]);
    // a frame of a flat sky, calibrated, is flat
    let sky: Vec<u16> = (0..W * H).map(|i| (200.0 + 20000.0 * vig(i % W, i / W)) as u16).collect();
    let out = cal.apply(&sky);
    let (c, e) = (out[120 * W + 160], out[5 * W + 5]);
    assert!((c - e).abs() / c < 0.05, "centre {c} corner {e}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sigma_clipping_removes_outliers_and_brightness_changes_are_evened_out() {
    let dir = std::env::temp_dir().join(format!("rf-stack-clip-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let base = scene(5);
    let mut r = Rng(21);
    // every frame the same scene (no jitter), noise, brightness varying by +-25 %, a hot pixel in one frame
    let frames: Vec<Vec<f32>> = (0..20)
        .map(|i| {
            let g = 1.0 + 0.25 * ((i % 5) as f32 - 2.0) / 2.0;
            let mut f: Vec<f32> = base.iter().map(|v| v * g + 200.0 * r.gauss()).map(|v| v.max(3.0)).collect();
            if i == 7 {
                f[120 * W + 160] = 65000.0; // a cosmic-ray hit on the disc
            }
            f
        })
        .collect();
    let path = dir.join("b.ser");
    write_ser(&path, &frames);
    let cal = Calibration::default();
    let plain = stack_ser(&path, &Options { keep: 1.0, ..Options::default() }, &cal).unwrap();
    let clipped = stack_ser(&path, &Options { keep: 1.0, clip: 2.5, ..Options::default() }, &cal).unwrap();
    let truth = base[120 * W + 160];
    let (p, c) = (plain.data[120 * W + 160], clipped.data[120 * W + 160]);
    assert!((p - truth).abs() > 1500.0, "the hit pulls the plain mean: {p} vs {truth}");
    assert!((c - truth).abs() < 600.0, "clipping removes it: {c} vs {truth}");
    assert!(clipped.report.clipped_fraction > 0.0 && clipped.report.clipped_fraction < 0.1);
    // brightness: the stack is at the median frame's level, not dragged by the extremes
    let level = |d: &[f32]| (60..H - 60).flat_map(|y| (60..W - 60).map(move |x| d[y * W + x] as f64)).sum::<f64>() / ((H - 120) * (W - 120)) as f64;
    let ratio = level(&plain.data) / level(&base);
    assert!((ratio - 1.0).abs() < 0.04, "level ratio {ratio}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_second_alignment_pass_converges() {
    let dir = std::env::temp_dir().join(format!("rf-stack-ref-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (frames, _) = burst(40);
    let path = dir.join("b.ser");
    write_ser(&path, &frames);
    let cal = Calibration::default();
    let two = stack_ser(&path, &Options::default(), &cal).unwrap();
    let one = stack_ser(&path, &Options { refine: 0.0, ..Options::default() }, &cal).unwrap();
    assert!(two.report.refine_change_px < 1.0, "the second pass moved the frames by {} px", two.report.refine_change_px);
    assert!(two.report.signal >= one.report.signal * 0.95, "refined {} vs single pass {}", two.report.signal, one.report.signal);
    let _ = std::fs::remove_dir_all(&dir);
}
