//! Developer benchmark of the per-frame analysis on a real full-resolution frame:
//! `RF_BENCH_PNG=path/to/frame.png cargo test --release -p rustflare-server bench_analysis -- --ignored --nocapture`
use rustflare_tracking::{locate_sun, Tracker};
use std::time::Instant;

fn time<T>(name: &str, n: u32, mut f: impl FnMut() -> T) {
    let _ = f();
    let t = Instant::now();
    for _ in 0..n {
        std::hint::black_box(f());
    }
    println!("{name:<34} {:>8.3} ms", t.elapsed().as_secs_f64() * 1000.0 / n as f64);
}

#[test]
#[ignore]
fn bench_analysis() {
    let Ok(path) = std::env::var("RF_BENCH_PNG") else { return };
    let frame = rustflare_capture::read_png(std::path::Path::new(&path)).expect("png");
    println!("frame {}x{} {:?}", frame.width, frame.height, frame.format);
    time("mean_fraction", 100, || frame.mean_fraction());
    time("saturated_fraction", 100, || frame.saturated_fraction());
    time("histogram(64)", 100, || frame.histogram(64));
    time("frame.clone()", 50, || frame.clone());
    time("locate_sun(hint 1228)", 30, || locate_sun(&frame, Some(1228.0)));
    time("locate_sun(no hint)", 30, || locate_sun(&frame, None));
    if let Ok(mut t) = Tracker::limb(&frame, Some(1228.0)) {
        time("tracker(limb).update", 30, || t.update(&frame));
    } else {
        println!("tracker(limb) could not start on this frame");
    }
}

/// Where the Sun's centre is in a series of raw frames (`RF_BENCH_DIR` holds `<name>.u16` 1944x1472 little-endian and `<name>.t` unix seconds).
#[test]
#[ignore]
fn bench_sun_positions() {
    let Ok(dir) = std::env::var("RF_BENCH_DIR") else { return };
    let mut names: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().map_or(false, |e| e == "u16")).collect();
    names.sort();
    let mut first: Option<(f64, f64, f64)> = None;
    for p in names {
        let data = std::fs::read(&p).unwrap();
        let frame = rustflare_core::Frame::new(1944, 1472, rustflare_core::PixelFormat::Mono16, data);
        let t: f64 = std::fs::read_to_string(p.with_extension("t")).unwrap().trim().parse().unwrap();
        let locked = locate_sun(&frame, Some(1228.46));
        let free = locate_sun(&frame, None);
        let f = |l: &Option<rustflare_tracking::SunLocation>| l.as_ref().map_or("none".to_string(), |s| format!("({:7.1},{:7.1}) r {:7.1}", s.pos.x, s.pos.y, s.radius));
        let t0 = *first.get_or_insert((t, 0.0, 0.0));
        println!("{} t+{:6.1}s  radius-locked {}  free {}  mean {:.3}", p.file_stem().unwrap().to_string_lossy().chars().skip(6).take(14).collect::<String>(), t - t0.0, f(&locked), f(&free), frame.mean_fraction());
    }
}
