//! The stacking pipeline for one recording: align every frame, rank them by how much real detail they carry, keep the best,
//! average.
//!
//! Ranking by the Laplacian variance of a single frame mostly ranks noise (the granulation and chromospheric texture are low
//! contrast), so the default score compares each aligned frame's high-pass detail with that of the recording's own mean: noise
//! does not correlate with it, real detail and seeing blur do.

use crate::align::{accumulate_shifted, Aligner};
use crate::calib::Calibration;
use crate::image::{bin, mask_f32, sharpness_masked, valid_mask, Img};
use crate::multipoint::{add_box, build_grid, LocalAligner, Patch};
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
    /// Sigma clipping of the mean: pixels further than this many standard deviations from the mean of the kept frames are left
    /// out (0 = off).
    pub clip: f32,
    /// Fraction of the frames whose aligned mean is the reference of the second alignment pass (0 = one pass only).
    pub refine: f64,
    /// Multi-point alignment (needs `align`): a shift and a frame selection per alignment point.
    pub multipoint: bool,
    /// Half-width of an alignment point's box, in binned pixels (the grid pitch is 1.5 times this).
    pub ap_half: usize,
    /// How far (binned pixels) a point may move from the global shift.
    pub search: usize,
    /// Fraction of the frames (the best by global score) whose points are measured.
    pub register: f64,
    /// A point needs this fraction of the 90th-percentile detail of all points.
    pub min_structure: f64,
}

impl Default for Options {
    fn default() -> Self { Options { keep: 0.25, align: true, threads: 2, bin: 2, whiten: 0.7, erode: 8, rank: Rank::Reference, cache_mb: 1024, integer_shifts: false, clip: 0.0, refine: 0.10, multipoint: true, ap_half: 24, search: 14, register: 0.5, min_structure: 0.05 } }
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
    /// How far the second alignment pass moved the frames from the first (full-resolution pixels, rms): small means the alignment had converged.
    pub refine_change_px: f64,
    /// Pixel values left out by sigma clipping, as a fraction of all.
    pub clipped_fraction: f64,
    /// Detail the two halves of the stack have in common (noise cancels): the quality measure to compare stacks with.
    pub signal: f64,
    /// The same measure for the global-shift stack, when multi-point alignment ran.
    pub signal_global: f64,
    /// Alignment points used (0 = global alignment only) and how far their shifts differ from the global one (full-resolution px, rms).
    pub aps: usize,
    pub local_shift_rms_px: f64,
    pub multipoint_seconds: f64,
    /// Seconds spent in each phase of the pipeline, in order.
    pub phases: Vec<(String, f64)>,
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
    let trace = std::env::var_os("RF_STACK_TRACE").is_some();
    let phase_log = std::cell::RefCell::new(Vec::<(String, f64)>::new());
    let lap_t = |what: &str, since: &mut Instant| {
        phase_log.borrow_mut().push((what.to_string(), (since.elapsed().as_secs_f64() * 1000.0).round() / 1000.0));
        if trace {
            eprintln!("  {what}: {:.2} s", since.elapsed().as_secs_f64());
        }
        *since = Instant::now();
    };
    let mut tp = Instant::now();
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
    let valid_f = mask_f32(&valid);
    let binned_of = |s: &mut Ser, i: usize| -> Result<Img, String> {
        let raw = s.read_frame(i).map_err(|e| e.to_string())?;
        Ok(cal.bin_calibrated(&raw, w, h, b))
    };

    // pass A: read every frame once, bin it, keep it in RAM when it fits, and take the Laplacian score
    let cache_ok = n * bw * bh * 4 <= opts.cache_mb * 1_000_000;
    let a = run_parallel(n, opts.threads, |i| {
        let mut s = Ser::open(path).map_err(|e| e.to_string())?;
        let img = binned_of(&mut s, i)?;
        let sh = sharpness_masked(&img, &valid_f);
        Ok::<_, String>((sh, if cache_ok { Some(img) } else { None }))
    });
    let mut lap = vec![0f64; n];
    let mut cache: Vec<Option<Img>> = Vec::with_capacity(n);
    for (i, r) in a.into_iter().enumerate() {
        let (sh, img) = r.map_err(|e| format!("frame {i}: {e}"))?;
        lap[i] = sh;
        cache.push(img);
    }
    lap_t("read, bin, score every frame (pass A)", &mut tp);
    let get = |s: &mut Ser, i: usize| -> Result<Img, String> { match &cache[i] { Some(img) => Ok(img.clone()), None => binned_of(s, i) } };
    let ref0 = (0..n).max_by(|&x, &y| lap[x].partial_cmp(&lap[y]).unwrap_or(std::cmp::Ordering::Equal)).unwrap_or(0);

    // With multi-point alignment the global shift only has to land within the points' search range, so it is found on 4x4-binned images
    // (four times less to transform) and not refined; the points do the fine work.
    let fast = opts.multipoint && opts.align && opts.clip == 0.0 && n >= 8;
    let (b4w, b4h) = (bw / 2, bh / 2);
    let valid4: Vec<bool> = (0..b4w * b4h).map(|i| { let (x, y) = (i % b4w * 2, i / b4w * 2); valid[y * bw + x] && valid[y * bw + x + 1] && valid[(y + 1) * bw + x] && valid[(y + 1) * bw + x + 1] }).collect();
    // alignment against a reference, then scoring against the aligned mean: shifts (binned pixels), score, brightness, aligned mean
    let aligner0 = Aligner::new(bw, bh, &valid, opts.whiten);
    let whole = |v: f64| if opts.integer_shifts { (v * b as f64).round() / b as f64 } else { v };
    let align_and_score = |reference: &crate::align::Prepared| -> Result<(Vec<(f64, f64)>, Vec<f64>, Vec<f64>, Vec<f32>), String> {
        let b_out = run_parallel(n, opts.threads, |i| {
            let mut s = Ser::open(path).map_err(|e| e.to_string())?;
            let img = get(&mut s, i)?;
            let (dx, dy) = if !opts.align {
                (0.0, 0.0)
            } else if fast {
                let al = Aligner::new(b4w, b4h, &valid4, opts.whiten);
                let (a, c) = al.shift(reference, &al.prepare(&bin(&img.d, bw, bh, 2)));
                (a * 2.0, c * 2.0)
            } else {
                let al = Aligner::new(bw, bh, &valid, opts.whiten);
                al.shift(reference, &al.prepare(&img))
            };
            Ok::<_, String>((whole(dx), whole(dy)))
        });
        let mut shifts = Vec::with_capacity(n);
        for (i, r) in b_out.into_iter().enumerate() {
            shifts.push(r.map_err(|e| format!("frame {i}: {e}"))?);
        }
        let mut mean_b = vec![0f32; bw * bh];
        {
            let mut s = Ser::open(path).map_err(|e| e.to_string())?;
            for i in 0..n {
                accumulate_shifted(&mut mean_b, &get(&mut s, i)?.d, bw, bh, shifts[i].0, shifts[i].1);
            }
        }
        for v in &mut mean_b {
            *v /= n as f32;
        }
        let lm = laplacian(&mean_b, bw, bh, &valid);
        let var_m: f64 = lm.iter().zip(&valid).filter(|(_, &v)| v).map(|(&l, _)| l as f64 * l as f64).sum::<f64>().max(1e-30);
        let nvalid = valid.iter().filter(|&&v| v).count().max(1) as f64;
        let c_out = run_parallel(n, opts.threads, |i| {
            let mut s = Ser::open(path).map_err(|e| e.to_string())?;
            let img = get(&mut s, i)?;
            let mut al = vec![0f32; bw * bh];
            accumulate_shifted(&mut al, &img.d, bw, bh, shifts[i].0, shifts[i].1);
            let l = laplacian(&al, bw, bh, &valid);
            let cov: f64 = l.iter().zip(&lm).zip(&valid).filter(|(_, &v)| v).map(|((&a, &b), _)| a as f64 * b as f64).sum();
            let mean_v = al.iter().zip(&valid).filter(|(_, &v)| v).map(|(&v, _)| v as f64).sum::<f64>() / nvalid;
            Ok::<_, String>((if mean_v > 0.0 { cov / var_m / mean_v } else { 0.0 }, mean_v)) // per unit brightness: dimmer frames are not penalised
        });
        let (mut score, mut bright) = (vec![0f64; n], vec![0f64; n]);
        for (i, r) in c_out.into_iter().enumerate() {
            (score[i], bright[i]) = r.map_err(|e| format!("frame {i}: {e}"))?;
        }
        Ok((shifts, score, bright, mean_b))
    };
    let by = |v: &Vec<f64>, o: &mut Vec<usize>| o.sort_by(|&x, &y| v[y].partial_cmp(&v[x]).unwrap_or(std::cmp::Ordering::Equal));

    // pass 1 against the sharpest single frame; pass 2 against the aligned mean of the best few (they agree with each other,
    // so the reference carries less noise and less of one frame's seeing)
    let reference = {
        let mut s = Ser::open(path).map_err(|e| e.to_string())?;
        let img = get(&mut s, ref0)?;
        if fast {
            Aligner::new(b4w, b4h, &valid4, opts.whiten).prepare(&bin(&img.d, bw, bh, 2))
        } else {
            aligner0.prepare(&img)
        }
    };
    let (mut shifts_b, mut score, mut bright, _) = align_and_score(&reference)?;
    lap_t("align and score against the best frame", &mut tp);
    let mut refine_change = 0.0;
    if opts.refine > 0.0 && opts.align && n >= 8 && !fast {
        let mut ord: Vec<usize> = (0..n).collect();
        by(&score, &mut ord);
        let top = ((n as f64 * opts.refine).round() as usize).clamp(3, n);
        let mut mean_top = vec![0f32; bw * bh];
        {
            let mut s = Ser::open(path).map_err(|e| e.to_string())?;
            for &i in &ord[..top] {
                accumulate_shifted(&mut mean_top, &get(&mut s, i)?.d, bw, bh, shifts_b[i].0, shifts_b[i].1);
            }
        }
        for v in &mut mean_top {
            *v /= top as f32;
        }
        let reference2 = aligner0.prepare(&Img { w: bw, h: bh, d: mean_top });
        let (shifts2, score2, bright2, _) = align_and_score(&reference2)?;
        refine_change = (shifts_b.iter().zip(&shifts2).map(|(a, c)| (a.0 - c.0).powi(2) + (a.1 - c.1).powi(2)).sum::<f64>() / n as f64).sqrt() * b as f64;
        (shifts_b, score, bright) = (shifts2, score2, bright2);
        lap_t("align and score again against the refined reference", &mut tp);
    }
    let mut order: Vec<usize> = (0..n).collect();
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

    // brightness of every kept frame brought to the median one (light changes during a burst)
    let target = {
        let mut v: Vec<f64> = kept.iter().map(|&i| bright[i]).collect();
        v.sort_by(|a, c| a.partial_cmp(c).unwrap_or(std::cmp::Ordering::Equal));
        v[v.len() / 2]
    };
    let scale = |i: usize| if bright[i] > 0.0 { (target / bright[i]).clamp(0.5, 2.0) as f32 } else { 1.0 };

    // pass D: the kept frames at full resolution, shifted onto the reference, into two accumulators (even and odd ranks) so
    // the quality of the stack can be measured without noise
    let t1 = Instant::now();
    let load = |my: &mut Ser, idx: usize| -> Option<Vec<f32>> {
        let raw = my.read_frame(idx).ok()?;
        let mut frame = cal.apply(&raw);
        let sc = scale(idx);
        if sc != 1.0 {
            for v in &mut frame {
                *v *= sc;
            }
        }
        Some(frame)
    };
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
                    let Some(frame) = load(&mut my, idx) else { continue };
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
    let mut data: Vec<f32> = even.iter().zip(&odd).map(|(e, o)| (e + o) / (ne + no).max(1) as f32).collect();
    let signal = if ne > 0 && no > 0 {
        let ea: Vec<f32> = even.iter().map(|v| v / ne as f32).collect();
        let oa: Vec<f32> = odd.iter().map(|v| v / no as f32).collect();
        let eval_valid = valid_mask(&bin(&mean, w, h, 2), 2.0, opts.erode);
        common_detail(&bin(&ea, w, h, 2).d, &bin(&oa, w, h, 2).d, w / 2, h / 2, &eval_valid)
    } else {
        0.0
    };

    // multi-point: each alignment point gets its own shift per frame and its own best frames
    let (mut aps_n, mut mp_rms, mut signal_global, mut mp_seconds) = (0usize, 0.0f64, signal, 0.0f64);
    let mut signal = signal;
    // sigma clipping works on the global-shift stack: asking for it turns multi-point off
    if opts.multipoint && opts.align && opts.clip == 0.0 && n >= 8 {
        let t2 = Instant::now();
        let mut ord: Vec<usize> = (0..n).collect();
        by(&score, &mut ord);
        let top = ((n as f64 * opts.refine.max(0.1)).round() as usize).clamp(3, n);
        let mut ref_d = vec![0f32; bw * bh];
        {
            let mut s = Ser::open(path).map_err(|e| e.to_string())?;
            for &i in &ord[..top] {
                accumulate_shifted(&mut ref_d, &get(&mut s, i)?.d, bw, bh, shifts_b[i].0, shifts_b[i].1);
            }
        }
        for v in &mut ref_d {
            *v /= top as f32;
        }
        let ref_img = Img { w: bw, h: bh, d: ref_d };
        let half = opts.ap_half.max(4);
        let grid = build_grid(&ref_img, &valid, half, (half * 3 / 2).max(1), opts.min_structure);
        if !grid.is_empty() {
            aps_n = grid.len();
            let la = LocalAligner::new(half, opts.search);
            let refp: Vec<Patch> = grid.iter().map(|&(x, y)| la.prepare(&ref_img.d, bw, bh, x as isize, y as isize)).collect();
            let reg_n = ((n as f64 * opts.register).round() as usize).clamp(kept_n, n);
            let registered: Vec<usize> = ord[..reg_n].to_vec();
            let local_r = run_parallel(reg_n, opts.threads, |k| {
                let i = registered[k];
                let mut s = Ser::open(path).map_err(|e| e.to_string())?;
                let img = get(&mut s, i)?;
                let (gx, gy) = (shifts_b[i].0.round() as isize, shifts_b[i].1.round() as isize);
                Ok::<_, String>(grid.iter().enumerate().map(|(j, &(x, y))| {
                    let cur = la.prepare(&img.d, bw, bh, x as isize + gx, y as isize + gy);
                    let (dx, dy, ncc) = la.correlate(&refp[j], &cur);
                    (gx as f32 + dx, gy as f32 + dy, ncc)
                }).collect::<Vec<_>>())
            });
            let mut local: Vec<Vec<(f32, f32, f32)>> = Vec::with_capacity(reg_n);
            for (k, r) in local_r.into_iter().enumerate() {
                local.push(r.map_err(|e| format!("frame {}: {e}", registered[k]))?);
            }
            // per point: the best `kept_n` frames by correlation, alternately into two halves (to measure the result)
            let per_ap = kept_n.min(reg_n);
            let mut chosen = vec![vec![0u8; aps_n]; reg_n];
            let (mut rs, mut rc) = (0.0f64, 0usize);
            for j in 0..aps_n {
                let mut idx: Vec<usize> = (0..reg_n).collect();
                idx.sort_by(|&a, &c| local[c][j].2.partial_cmp(&local[a][j].2).unwrap_or(std::cmp::Ordering::Equal));
                for (rank, &k) in idx[..per_ap].iter().enumerate() {
                    chosen[k][j] = if rank % 2 == 0 { 1 } else { 2 };
                    let g = shifts_b[registered[k]];
                    rs += ((local[k][j].0 as f64 - g.0).powi(2) + (local[k][j].1 as f64 - g.1).powi(2)) as f64;
                    rc += 1;
                }
            }
            mp_rms = (rs / rc.max(1) as f64).sqrt() * b as f64;
            let accs = Mutex::new(Vec::<[(Vec<f32>, Vec<f32>); 2]>::new());
            let next = AtomicUsize::new(0);
            std::thread::scope(|sc| {
                for _ in 0..opts.threads.max(1).min(reg_n) {
                    sc.spawn(|| {
                        let mut my = Ser::open(path).expect("reopen");
                        let mut st = [(vec![0f32; w * h], vec![0f32; w * h]), (vec![0f32; w * h], vec![0f32; w * h])];
                        loop {
                            let k = next.fetch_add(1, Ordering::Relaxed);
                            if k >= reg_n {
                                break;
                            }
                            if chosen[k].iter().all(|&c| c == 0) {
                                continue;
                            }
                            let Some(frame) = load(&mut my, registered[k]) else { continue };
                            for (j, &(x, y)) in grid.iter().enumerate() {
                                let c = chosen[k][j];
                                if c == 0 {
                                    continue;
                                }
                                let (dx, dy, _) = local[k][j];
                                let g = &mut st[(c - 1) as usize];
                                add_box(&mut g.0, &mut g.1, &frame, w, h, (x * b) as isize, (y * b) as isize, half * b, dx * b as f32, dy * b as f32);
                            }
                        }
                        accs.lock().unwrap().push(st);
                    });
                }
            });
            let mut sums = [(vec![0f32; w * h], vec![0f32; w * h]), (vec![0f32; w * h], vec![0f32; w * h])];
            for st in accs.into_inner().unwrap() {
                for (g, part) in sums.iter_mut().zip(st) {
                    for (d, v) in g.0.iter_mut().zip(part.0) { *d += v; }
                    for (d, v) in g.1.iter_mut().zip(part.1) { *d += v; }
                }
            }
            // where no point reaches, the global stack shows through
            let e = 0.02 * per_ap as f32;
            let blend = |num: &[f32], den: &[f32], e: f32| -> Vec<f32> { (0..w * h).map(|i| (num[i] + e * data[i]) / (den[i] + e)).collect() };
            let a = blend(&sums[0].0, &sums[0].1, e / 2.0);
            let bb = blend(&sums[1].0, &sums[1].1, e / 2.0);
            let both_num: Vec<f32> = sums[0].0.iter().zip(&sums[1].0).map(|(x, y)| x + y).collect();
            let both_den: Vec<f32> = sums[0].1.iter().zip(&sums[1].1).map(|(x, y)| x + y).collect();
            signal = {
                let eval_valid = valid_mask(&bin(&mean, w, h, 2), 2.0, opts.erode);
                common_detail(&bin(&a, w, h, 2).d, &bin(&bb, w, h, 2).d, w / 2, h / 2, &eval_valid)
            };
            data = blend(&both_num, &both_den, e);
        }
        mp_seconds = t2.elapsed().as_secs_f64();
    }
    lap_t("multi-point", &mut tp);
    drop(cache);
    signal_global = if aps_n > 0 { signal_global } else { signal };

    // sigma clipping: the spread of the kept frames about their mean, then the mean without the outliers (hot pixels, cosmic
    // rays, a frame with a bird in it)
    lap_t("global stack of the kept frames (pass D)", &mut tp);
    let mut clipped_fraction = 0.0;
    if opts.clip > 0.0 && kept_n >= 5 && aps_n == 0 {
        let mean_px = data.clone();
        let run = |pass: u8, std: &[f32]| -> Vec<(Vec<f32>, Vec<f32>)> {
            let out = Mutex::new(Vec::new());
            let next = AtomicUsize::new(0);
            std::thread::scope(|s| {
                for _ in 0..opts.threads.max(1).min(kept_n) {
                    s.spawn(|| {
                        let mut my = Ser::open(path).expect("reopen");
                        let (mut a, mut c) = (vec![0f32; w * h], vec![0f32; w * h]);
                        let mut sh = vec![0f32; w * h];
                        loop {
                            let k = next.fetch_add(1, Ordering::Relaxed);
                            if k >= kept_n {
                                break;
                            }
                            let idx = kept[k];
                            let Some(frame) = load(&mut my, idx) else { continue };
                            sh.iter_mut().for_each(|v| *v = 0.0);
                            accumulate_shifted(&mut sh, &frame, w, h, shifts_b[idx].0 * b as f64, shifts_b[idx].1 * b as f64);
                            for i in 0..w * h {
                                let d = sh[i] - mean_px[i];
                                if pass == 0 {
                                    a[i] += d * d;
                                } else if std[i] <= 0.0 || d.abs() <= opts.clip * std[i] {
                                    a[i] += sh[i];
                                    c[i] += 1.0;
                                }
                            }
                        }
                        out.lock().unwrap().push((a, c));
                    });
                }
            });
            out.into_inner().unwrap()
        };
        let mut var = vec![0f32; w * h];
        for (a, _) in run(0, &[]) {
            for (v, x) in var.iter_mut().zip(a) { *v += x; }
        }
        let std: Vec<f32> = var.iter().map(|v| (v / (kept_n - 1) as f32).sqrt()).collect();
        let (mut sum, mut cnt) = (vec![0f32; w * h], vec![0f32; w * h]);
        for (a, c) in run(1, &std) {
            for i in 0..w * h { sum[i] += a[i]; cnt[i] += c[i]; }
        }
        let total: f64 = cnt.iter().map(|&c| c as f64).sum();
        clipped_fraction = 1.0 - total / (w * h * kept_n) as f64;
        for i in 0..w * h {
            if cnt[i] > 0.0 { data[i] = sum[i] / cnt[i]; }
        }
    }

    let shifts: Vec<(usize, f64, f64)> = kept.iter().map(|&i| (i, shifts_b[i].0 * b as f64, shifts_b[i].1 * b as f64)).collect();
    let rms = (shifts.iter().map(|s| s.1 * s.1 + s.2 * s.2).sum::<f64>() / shifts.len().max(1) as f64).sqrt();
    let report = Report {
        frames: n,
        kept: kept_n,
        score,
        kept_frames: kept,
        shifts,
        shift_rms_px: rms,
        refine_change_px: refine_change,
        clipped_fraction,
        signal,
        signal_global,
        aps: aps_n,
        local_shift_rms_px: mp_rms,
        multipoint_seconds: mp_seconds,
        phases: phase_log.into_inner(),
        first_timestamp: ser.timestamp(0),
        rank_seconds,
        stack_seconds: t1.elapsed().as_secs_f64(),
        flat_applied: cal.flat.is_some(),
        dark_applied: cal.dark.is_some(),
    };
    Ok(Stacked { width: w, height: h, data, report })
}
