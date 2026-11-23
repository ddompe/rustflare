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
    assert!(best.report.signal > unaligned.report.signal * 1.2, "aligned {:.6} vs unaligned {:.6}", best.report.signal, unaligned.report.signal);
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

/// `base` bent by a smooth displacement field: out(x, y) = base(x - u, y - v), with u, v of a few pixels varying across the image
/// (what seeing does: patches a few arcseconds apart move independently).
fn warped(base: &[f32], a: f32, b: f32, p: f32, q: f32, jitter: (f32, f32)) -> Vec<f32> {
    let at = |x: isize, y: isize| base[(y.clamp(0, H as isize - 1) as usize) * W + x.clamp(0, W as isize - 1) as usize];
    let mut out = vec![0f32; W * H];
    for y in 0..H {
        for x in 0..W {
            let u = jitter.0 + a * (y as f32 / 45.0 + p).sin();
            let v = jitter.1 + b * (x as f32 / 50.0 + q).sin();
            let (sx, sy) = (x as f32 - u, y as f32 - v);
            let (ix, iy) = (sx.floor(), sy.floor());
            let (fx, fy) = (sx - ix, sy - iy);
            let (ix, iy) = (ix as isize, iy as isize);
            out[y * W + x] = at(ix, iy) * (1.0 - fx) * (1.0 - fy) + at(ix + 1, iy) * fx * (1.0 - fy) + at(ix, iy + 1) * (1.0 - fx) * fy + at(ix + 1, iy + 1) * fx * fy;
        }
    }
    out
}

#[test]
fn multipoint_follows_local_distortion_that_one_shift_per_frame_cannot() {
    let dir = std::env::temp_dir().join(format!("rf-stack-mp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let base = scene(13);
    let mut r = Rng(77);
    let frames: Vec<Vec<f32>> = (0..60)
        .map(|_| {
            let mut f = warped(&base, 2.5 * r.gauss(), 2.5 * r.gauss(), 6.0 * r.next(), 6.0 * r.next(), (r.gauss(), r.gauss()));
            for v in &mut f {
                *v = (*v + 200.0 * r.gauss()).max(3.0);
            }
            f
        })
        .collect();
    let path = dir.join("w.ser");
    write_ser(&path, &frames);
    let cal = Calibration::default();
    let small = Options { ap_half: 8, search: 10, min_structure: 0.02, keep: 0.5, register: 1.0, ..Options::default() };
    let global = stack_ser(&path, &Options { multipoint: false, ..small.clone() }, &cal).unwrap();
    let multi = stack_ser(&path, &small, &cal).unwrap();
    assert!(multi.report.aps > 4, "{} points", multi.report.aps);
    assert!(multi.report.local_shift_rms_px > 0.4, "the points moved by {} px from the global shift", multi.report.local_shift_rms_px);
    // both stacks live in the (distorted) coordinates of the reference frames, so compare them by the detail their two halves
    // share (noise and the reference's own distortion cancel), not against the true scene
    assert!(multi.report.signal > multi.report.signal_global * 1.08, "split-half detail {} vs {}", multi.report.signal, multi.report.signal_global);
    assert_eq!(global.report.aps, 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn local_aligner_measures_the_displacement_at_each_point() {
    use rustflare_stack::multipoint::LocalAligner;
    let base = scene(17);
    let (a, b, p, q) = (3.0f32, -2.0f32, 1.0f32, 2.0f32);
    let img = warped(&base, a, b, p, q, (0.5, -0.5));
    let la = LocalAligner::new(16, 10);
    let mut worst = 0.0f32;
    let mut n = 0;
    for &(cx, cy) in &[(110usize, 100usize), (160, 120), (200, 90), (130, 150), (190, 140)] {
        let r = la.prepare(&base, W, H, cx as isize, cy as isize);
        let i = la.prepare(&img, W, H, cx as isize, cy as isize);
        let (dx, dy, ncc) = la.correlate(&r, &i);
        // the true displacement at this point (see `warped`): img(x) = base(x - u)
        let u = 0.5 + a * (cy as f32 / 45.0 + p).sin();
        let v = -0.5 + b * (cx as f32 / 50.0 + q).sin();
        worst = worst.max((dx - u).abs()).max((dy - v).abs());
        assert!(ncc > 0.5, "correlation {ncc} at ({cx}, {cy})");
        n += 1;
    }
    assert!(worst < 0.5, "worst error {worst} px over {n} points");
}

fn as_u16(frames: &[Vec<f32>]) -> Vec<Vec<u16>> { frames.iter().map(|f| f.iter().map(|&v| v.clamp(0.0, 65535.0) as u16).collect()).collect() }

fn to_ring(frames: &[Vec<u16>], slots: usize) -> (rustflare_ring::Ring, std::os::fd::OwnedFd) {
    let (ring, fd) = rustflare_ring::Ring::create(rustflare_ring::Geometry { width: W as u32, height: H as u32, bits: 16, format: 0 }, slots).unwrap();
    let mut p = ring.become_producer();
    for (i, f) in frames.iter().enumerate() {
        let bytes: Vec<u8> = f.iter().flat_map(|v| v.to_le_bytes()).collect();
        p.push(&bytes, rustflare_ring::FrameMeta { ts_ns: 1_700_000_000_000_000_000 + i as u64 * 1_000_000, ..Default::default() }).unwrap();
    }
    (ring, fd)
}

#[test]
fn a_stack_from_memory_or_from_the_ring_is_the_same_computation_as_from_the_file() {
    use rustflare_stack::source::{MemFrames, RingFrames};
    use rustflare_stack::stack::stack_frames;
    let dir = std::env::temp_dir().join(format!("rf-stack-src-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (fl, _) = burst(40);
    let frames = as_u16(&fl);
    let path = dir.join("b.ser");
    write_ser(&path, &fl);
    let cal = Calibration::default();
    let o = Options { threads: 1, ..Options::default() }; // one thread: the order of the float sums is then fixed
    let from_file = stack_ser(&path, &o, &cal).unwrap();
    let from_mem = stack_frames(&MemFrames { width: W, height: H, frames: frames.clone(), first_timestamp: None }, &o, &cal).unwrap();
    let (ring, _fd) = to_ring(&frames, 64);
    let consumer = ring.attach_consumer("stacker", false, false).unwrap();
    let from_ring = stack_frames(&RingFrames::new(consumer, 1, frames.len()).unwrap(), &o, &cal).unwrap();
    for (name, s) in [("memory", &from_mem), ("ring", &from_ring)] {
        assert_eq!(s.data, from_file.data, "the stack from {name} differs from the one from the file");
        assert_eq!((s.report.kept_frames.clone(), s.report.aps), (from_file.report.kept_frames.clone(), from_file.report.aps), "{name}");
        assert_eq!(s.report.signal, from_file.report.signal, "{name}");
    }
    // the ring knows when the first frame was taken; the stack carries it
    assert!(from_ring.report.first_timestamp.unwrap() > 1.6e9);
    // and the stacker shows up in the ring's consumer list while it works (advisory pin), and is gone after
    assert!(ring.consumers().iter().all(|c| c.pin_seq == 0), "the pin is released when the source is dropped");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_ring_frame_overwritten_before_or_during_the_stack_is_an_error_and_never_a_silent_gap() {
    use rustflare_stack::source::RingFrames;
    use rustflare_stack::stack::stack_frames;
    let (fl, _) = burst(24);
    let frames = as_u16(&fl);
    // a ring too small for the burst: the first frames are already gone when the stacker starts
    let (small, _fd) = to_ring(&frames, 16);
    let c = small.attach_consumer("stacker", false, false).unwrap();
    let err = RingFrames::new(c, 1, frames.len()).err().unwrap();
    assert!(err.contains("overwritten"), "{err}");
    // a ring that held the burst at first and is then overwritten by the camera while the stacker has not finished
    let (ring, _fd2) = to_ring(&frames, 24);
    let c = ring.attach_consumer("stacker", false, false).unwrap();
    let src = RingFrames::new(c, 1, frames.len()).unwrap();
    assert!(src.intact());
    let mut p = ring.become_producer();
    let bytes: Vec<u8> = frames[0].iter().flat_map(|v| v.to_le_bytes()).collect();
    for _ in 0..6 { p.push(&bytes, Default::default()).unwrap(); } // the camera wraps over frames 1 to 6
    assert!(!src.intact());
    let e = stack_frames(&src, &Options::default(), &Calibration::default()).err().unwrap();
    assert!(e.contains("overwritten"), "the failure says why: {e}");
}
