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
