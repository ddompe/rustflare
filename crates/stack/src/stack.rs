//! The stacking pipeline for one recording: align every frame, rank them by how much real detail they carry, keep the best,
//! average.
//!
//! Ranking by the Laplacian variance of a single frame mostly ranks noise (the granulation and chromospheric texture are low
//! contrast), so the default score compares each aligned frame's high-pass detail with that of the recording's own mean: noise
//! does not correlate with it, real detail and seeing blur do.

use crate::align::{accumulate_shifted, Aligner};
use crate::calib::Calibration;
use crate::image::{bin, sharpness, valid_mask, Img};
use crate::ser::Ser;
use serde::Serialize;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rank {
    /// Correlation of the aligned frame's detail with the recording's mean (default).
    Reference,
    /// Laplacian variance of the single frame (noise sensitive; for comparison).
    Laplacian,
    /// File order: the first frames (for comparison).
    Order,
    /// The worst frames by the reference score (for comparison and tests).
    Worst,
}

#[derive(Clone, Debug)]
pub struct Options {
    /// Fraction of the frames to keep, the best first (0 to 1).
    pub keep: f64,
    pub align: bool,
    pub threads: usize,
    /// Binning of the images used for ranking and alignment.
    pub bin: usize,
    /// 0 = plain cross-correlation, 1 = full phase correlation.
    pub whiten: f32,
    /// Pixels (after binning) to stay away from a masked area's edge.
    pub erode: usize,
    pub rank: Rank,
    /// RAM for the binned frames, so the passes over the recording do not read it again (MB).
    pub cache_mb: usize,
    /// Shift by whole pixels only: no resampling blur, at most half a pixel of misalignment.
    pub integer_shifts: bool,
}

impl Default for Options {
    fn default() -> Self { Options { keep: 0.25, align: true, threads: 2, bin: 2, whiten: 0.7, erode: 8, rank: Rank::Reference, cache_mb: 1024, integer_shifts: false } }
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Report {
    pub frames: usize,
    pub kept: usize,
    /// The ranking score of every frame, in file order (higher is better).
    pub score: Vec<f64>,
    /// Indices of the frames kept, best first.
    pub kept_frames: Vec<usize>,
    /// Shift of each frame from the reference frame, in full-resolution pixels: (frame, dx, dy), kept frames only.
    pub shifts: Vec<(usize, f64, f64)>,
    pub shift_rms_px: f64,
    /// Detail the two halves of the stack have in common (noise cancels): the quality measure to compare stacks with.
    pub signal: f64,
    pub first_timestamp: Option<f64>,
    pub rank_seconds: f64,
    pub stack_seconds: f64,
    pub flat_applied: bool,
    pub dark_applied: bool,
}

pub struct Stacked {
    pub width: usize,
    pub height: usize,
    pub data: Vec<f32>,
    pub report: Report,
}

fn run_parallel<T: Send>(n: usize, threads: usize, work: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let next = AtomicUsize::new(0);
    let out = Mutex::new((0..n).map(|_| None).collect::<Vec<Option<T>>>());
    std::thread::scope(|s| {
        for _ in 0..threads.max(1).min(n.max(1)) {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= n {
                    break;
                }
                let r = work(i);
                out.lock().unwrap()[i] = Some(r);
            });
        }
    });
    out.into_inner().unwrap().into_iter().map(|o| o.unwrap()).collect()
}

fn laplacian(img: &[f32], w: usize, h: usize, valid: &[bool]) -> Vec<f32> {
    let mut l = vec![0f32; w * h];
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let i = y * w + x;
            if valid[i] {
                l[i] = 4.0 * img[i] - img[i - 1] - img[i + 1] - img[i - w] - img[i + w];
            }
        }
    }
    l
}

/// Detail two images have in common, per squared mean brightness.
fn common_detail(a: &[f32], b: &[f32], w: usize, h: usize, valid: &[bool]) -> f64 {
    let (la, lb) = (laplacian(a, w, h, valid), laplacian(b, w, h, valid));
    let (mut s, mut n, mut m) = (0.0f64, 0usize, 0.0f64);
    for i in 0..w * h {
        if valid[i] {
            s += la[i] as f64 * lb[i] as f64;
            m += (a[i] + b[i]) as f64 * 0.5;
            n += 1;
        }
    }
    if n == 0 || m <= 0.0 { return 0.0; }
    let mean = m / n as f64;
    s / n as f64 / (mean * mean)
}

/// Stack one SER recording (with its `_partN` files).
pub fn stack_ser(path: &Path, opts: &Options, cal: &Calibration) -> Result<Stacked, String> {
    let mut ser = Ser::open(path).map_err(|e| e.to_string())?;
    let (w, h, n) = (ser.width, ser.height, ser.frames());
    if n == 0 {
        return Err(format!("{}: no frames", path.display()));
    }
    if ser.is_mosaic() {
        return Err("colour (mosaic) recordings are not supported yet".into());
    }
    if let Some(d) = cal.dark.as_ref().or(cal.flat.as_ref()) {
        if d.len() != w * h {
            return Err(format!("the calibration frames are {}x{} but the recording is {w}x{h}", cal.width, cal.height));
        }
    }
    let t0 = Instant::now();
    let b = opts.bin.max(1);
    let (bw, bh) = (w / b, h / b);

    // which pixels carry signal: from a handful of raw frames spread over the recording (a coronagraph mask is exactly black)
    let probe: Vec<usize> = (0..n.min(6)).map(|k| k * n / n.min(6)).collect();
    let mut mean = vec![0f32; w * h];
    for &i in &probe {
        for (m, v) in mean.iter_mut().zip(ser.read_frame(i).map_err(|e| e.to_string())?) {
            *m += v as f32 / probe.len() as f32;
        }
    }
    let valid = valid_mask(&bin(&mean, w, h, b), 2.0, opts.erode);
    let binned_of = |s: &mut Ser, i: usize| -> Result<Img, String> {
        let raw = s.read_frame(i).map_err(|e| e.to_string())?;
        Ok(bin(&cal.apply(&raw), w, h, b))
    };

    // pass A: read every frame once, bin it, keep it in RAM when it fits, and take the Laplacian score
    let cache_ok = n * bw * bh * 4 <= opts.cache_mb * 1_000_000;
    let a = run_parallel(n, opts.threads, |i| {
        let mut s = Ser::open(path).map_err(|e| e.to_string())?;
        let img = binned_of(&mut s, i)?;
        let sh = sharpness(&img, &valid);
        Ok::<_, String>((sh, if cache_ok { Some(img) } else { None }))
    });
    let mut lap = vec![0f64; n];
    let mut cache: Vec<Option<Img>> = Vec::with_capacity(n);
    for (i, r) in a.into_iter().enumerate() {
        let (sh, img) = r.map_err(|e| format!("frame {i}: {e}"))?;
        lap[i] = sh;
        cache.push(img);
    }
    let get = |s: &mut Ser, i: usize| -> Result<Img, String> { match &cache[i] { Some(img) => Ok(img.clone()), None => binned_of(s, i) } };
    let ref0 = (0..n).max_by(|&x, &y| lap[x].partial_cmp(&lap[y]).unwrap_or(std::cmp::Ordering::Equal)).unwrap_or(0);

    // pass B: the shift of every frame from the reference, and the aligned mean
    let aligner0 = Aligner::new(bw, bh, &valid, opts.whiten);
    let reference = {
        let mut s = Ser::open(path).map_err(|e| e.to_string())?;
        aligner0.prepare(&get(&mut s, ref0)?)
    };
    let b_out = run_parallel(n, opts.threads, |i| {
        let mut s = Ser::open(path).map_err(|e| e.to_string())?;
        let img = get(&mut s, i)?;
        let al = Aligner::new(bw, bh, &valid, opts.whiten);
        let (dx, dy) = if opts.align && i != ref0 { al.shift(&reference, &al.prepare(&img)) } else { (0.0, 0.0) };
        Ok::<_, String>((dx, dy))
    });
    let mut shifts_b = Vec::with_capacity(n);
    for (i, r) in b_out.into_iter().enumerate() {
        shifts_b.push(r.map_err(|e| format!("frame {i}: {e}"))?);
    }
    let whole = |v: f64| if opts.integer_shifts { (v * b as f64).round() / b as f64 } else { v };
    for sh in &mut shifts_b {
        *sh = (whole(sh.0), whole(sh.1));
    }
    let mut mean_b = vec![0f32; bw * bh];
    {
        let mut s = Ser::open(path).map_err(|e| e.to_string())?;
        for i in 0..n {
            let img = get(&mut s, i)?;
            accumulate_shifted(&mut mean_b, &img.d, bw, bh, shifts_b[i].0, shifts_b[i].1);
        }
    }
    for v in &mut mean_b {
        *v /= n as f32;
    }

    // pass C: how well each aligned frame's detail agrees with the mean's
    let lm = laplacian(&mean_b, bw, bh, &valid);
    let var_m: f64 = lm.iter().zip(&valid).filter(|(_, &v)| v).map(|(&l, _)| l as f64 * l as f64).sum::<f64>().max(1e-30);
    let c_out = run_parallel(n, opts.threads, |i| {
        let mut s = Ser::open(path).map_err(|e| e.to_string())?;
        let img = get(&mut s, i)?;
        let mut al = vec![0f32; bw * bh];
        accumulate_shifted(&mut al, &img.d, bw, bh, shifts_b[i].0, shifts_b[i].1);
        let l = laplacian(&al, bw, bh, &valid);
        let cov: f64 = l.iter().zip(&lm).zip(&valid).filter(|(_, &v)| v).map(|((&a, &b), _)| a as f64 * b as f64).sum();
        let mean_v = al.iter().zip(&valid).filter(|(_, &v)| v).map(|(&v, _)| v as f64).sum::<f64>() / valid.iter().filter(|&&v| v).count().max(1) as f64;
        Ok::<_, String>(if mean_v > 0.0 { cov / var_m / mean_v } else { 0.0 }) // per unit brightness: dimmer frames are not penalised
    });
    let mut score = vec![0f64; n];
    for (i, r) in c_out.into_iter().enumerate() {
        score[i] = r.map_err(|e| format!("frame {i}: {e}"))?;
    }
    drop(cache);

    let mut order: Vec<usize> = (0..n).collect();
    let by = |v: &Vec<f64>, o: &mut Vec<usize>| o.sort_by(|&x, &y| v[y].partial_cmp(&v[x]).unwrap_or(std::cmp::Ordering::Equal));
    match opts.rank {
        Rank::Reference => by(&score, &mut order),
        Rank::Laplacian => by(&lap, &mut order),
        Rank::Order => {}
        Rank::Worst => {
            by(&score, &mut order);
            order.reverse();
        }
    }
    let kept_n = ((n as f64 * opts.keep).round() as usize).clamp(1, n);
    let kept: Vec<usize> = order[..kept_n].to_vec();
    let rank_seconds = t0.elapsed().as_secs_f64();

    // pass D: the kept frames at full resolution, shifted onto the reference, into two accumulators (even and odd ranks) so
    // the quality of the stack can be measured without noise
    let t1 = Instant::now();
    let accs = Mutex::new(Vec::<(Vec<f32>, Vec<f32>, usize, usize)>::new());
    let next = AtomicUsize::new(0);
    std::thread::scope(|s| {
        for _ in 0..opts.threads.max(1).min(kept_n) {
            s.spawn(|| {
                let mut my = Ser::open(path).expect("reopen");
                let (mut even, mut odd, mut ne, mut no) = (vec![0f32; w * h], vec![0f32; w * h], 0, 0);
                loop {
                    let k = next.fetch_add(1, Ordering::Relaxed);
                    if k >= kept_n {
                        break;
                    }
                    let idx = kept[k];
                    let Ok(raw) = my.read_frame(idx) else { continue };
                    let frame = cal.apply(&raw);
                    let (fx, fy) = (shifts_b[idx].0 * b as f64, shifts_b[idx].1 * b as f64);
                    if k % 2 == 0 {
                        accumulate_shifted(&mut even, &frame, w, h, fx, fy);
                        ne += 1;
                    } else {
                        accumulate_shifted(&mut odd, &frame, w, h, fx, fy);
                        no += 1;
                    }
                }
                accs.lock().unwrap().push((even, odd, ne, no));
            });
        }
    });
    let (mut even, mut odd, mut ne, mut no) = (vec![0f32; w * h], vec![0f32; w * h], 0usize, 0usize);
    for (e, o, a, c) in accs.into_inner().unwrap() {
        for (d, v) in even.iter_mut().zip(e) { *d += v; }
        for (d, v) in odd.iter_mut().zip(o) { *d += v; }
        ne += a;
        no += c;
    }
    let data: Vec<f32> = even.iter().zip(&odd).map(|(e, o)| (e + o) / kept_n as f32).collect();
    let signal = if ne > 0 && no > 0 {
        let ea: Vec<f32> = even.iter().map(|v| v / ne as f32).collect();
        let oa: Vec<f32> = odd.iter().map(|v| v / no as f32).collect();
        let eval_valid = valid_mask(&bin(&mean, w, h, 2), 2.0, opts.erode);
        common_detail(&bin(&ea, w, h, 2).d, &bin(&oa, w, h, 2).d, w / 2, h / 2, &eval_valid)
    } else {
        0.0
    };
    let shifts: Vec<(usize, f64, f64)> = kept.iter().map(|&i| (i, shifts_b[i].0 * b as f64, shifts_b[i].1 * b as f64)).collect();
    let rms = (shifts.iter().map(|s| s.1 * s.1 + s.2 * s.2).sum::<f64>() / shifts.len().max(1) as f64).sqrt();
    let report = Report {
        frames: n,
        kept: kept_n,
        score,
        kept_frames: kept,
        shifts,
        shift_rms_px: rms,
        signal,
        first_timestamp: ser.timestamp(0),
        rank_seconds,
        stack_seconds: t1.elapsed().as_secs_f64(),
        flat_applied: cal.flat.is_some(),
        dark_applied: cal.dark.is_some(),
    };
    Ok(Stacked { width: w, height: h, data, report })
}
