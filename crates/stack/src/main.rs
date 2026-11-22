//! `rustflare-stack stack <file.ser>... [options]`: rank, align and stack recordings into FITS stills.

use rustflare_stack::{calib::Calibration, fits, stack::{stack_ser, Options, Rank}};
use std::path::PathBuf;

const USAGE: &str = "usage: rustflare-stack stack <file.ser>... [--keep 25] [--out DIR] [--flat master.png] [--dark master.png]
       [--no-align] [--rank reference|laplacian|order|worst] [--float] [--threads N] [--bin N] [--whiten 0..1] [--exposure-us N --gain N] [--json]
  --keep      percent of the frames to keep, the sharpest first (default 25)
  --out       directory for the stacks (default: next to each recording, in stacks/)
  --float     32-bit float FITS instead of unsigned 16-bit
  --json      print each report as JSON";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(|s| s.as_str()) != Some("stack") || args.len() < 2 {
        eprintln!("{USAGE}");
        std::process::exit(2);
    }
    let (mut files, mut out, mut flat, mut dark) = (Vec::<PathBuf>::new(), None::<PathBuf>, None::<PathBuf>, None::<PathBuf>);
    let (mut opts, mut float, mut json, mut print_shifts) = (Options::default(), false, false, false);
    let (mut exposure, mut gain) = (None::<f64>, None::<f64>);
    let mut i = 1;
    let val = |i: &mut usize| -> String {
        *i += 1;
        args.get(*i).cloned().unwrap_or_else(|| { eprintln!("{USAGE}"); std::process::exit(2) })
    };
    while i < args.len() {
        match args[i].as_str() {
            "--keep" => opts.keep = val(&mut i).parse::<f64>().unwrap_or(25.0) / 100.0,
            "--out" => out = Some(val(&mut i).into()),
            "--flat" => flat = Some(val(&mut i).into()),
            "--dark" => dark = Some(val(&mut i).into()),
            "--no-align" => opts.align = false,
            "--float" => float = true,
            "--integer-shifts" => opts.integer_shifts = true,
            "--shifts" => print_shifts = true,
            "--json" => json = true,
            "--threads" => opts.threads = val(&mut i).parse().unwrap_or(2),
            "--bin" => opts.bin = val(&mut i).parse().unwrap_or(2),
            "--rank" => opts.rank = match val(&mut i).as_str() { "laplacian" => Rank::Laplacian, "order" => Rank::Order, "worst" => Rank::Worst, _ => Rank::Reference },
            "--whiten" => opts.whiten = val(&mut i).parse().unwrap_or(0.7),
            "--exposure-us" => exposure = val(&mut i).parse().ok(),
            "--gain" => gain = val(&mut i).parse().ok(),
            f if !f.starts_with("--") => files.push(f.into()),
            other => {
                eprintln!("unknown option {other}\n{USAGE}");
                std::process::exit(2);
            }
        }
        i += 1;
    }
    let cal = match Calibration::load(dark.as_deref(), flat.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("calibration: {e}");
            std::process::exit(1);
        }
    };
    let mut failed = false;
    for f in &files {
        let dir = out.clone().unwrap_or_else(|| f.parent().unwrap_or(std::path::Path::new(".")).join("stacks"));
        let _ = std::fs::create_dir_all(&dir);
        match stack_ser(f, &opts, &cal) {
            Err(e) => {
                eprintln!("{}: {e}", f.display());
                failed = true;
            }
            Ok(s) => {
                let stem = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "stack".into());
                let target = dir.join(format!("stack_{stem}.fits"));
                let r = &s.report;
                let mut extra: Vec<fits::Extra> = vec![
                    ("STACKED".into(), r.kept.to_string(), "frames averaged".into()),
                    ("NFRAMES".into(), r.frames.to_string(), "frames in the recording".into()),
                    ("KEEPFRAC".into(), format!("{:.3}", opts.keep), "fraction kept, sharpest first".into()),
                    ("ALIGNED".into(), if opts.align { "T" } else { "F" }.into(), "phase-correlation alignment".into()),
                    ("FLATCORR".into(), if r.flat_applied { "T" } else { "F" }.into(), "master flat applied".into()),
                    ("DARKCORR".into(), if r.dark_applied { "T" } else { "F" }.into(), "master dark applied".into()),
                    ("ORIGFILE".into(), fits::quoted(&f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()), "source recording".into()),
                ];
                if let Some(t) = r.first_timestamp {
                    let secs = t as i64;
                    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
                    let z = days + 719468;
                    let era = z.div_euclid(146097);
                    let doe = z - era * 146097;
                    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
                    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
                    let mp = (5 * doy + 2) / 153;
                    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
                    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
                    extra.push(("DATE-OBS".into(), fits::quoted(&format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}", rem / 3600, rem % 3600 / 60, rem % 60)), "first frame, UTC".into()));
                }
                if let Some(e) = exposure {
                    extra.push(("EXPTIME".into(), format!("{:.6}", e / 1e6), "seconds per frame".into()));
                }
                if let Some(g) = gain {
                    extra.push(("GAIN".into(), format!("{g}"), "camera gain".into()));
                }
                if let Err(e) = fits::write(&target, s.width, s.height, &s.data, float, &extra) {
                    eprintln!("{}: {e}", target.display());
                    failed = true;
                    continue;
                }
                if print_shifts {
                    let mut v: Vec<_> = r.shifts.clone();
                    v.sort_by_key(|s| s.0);
                    println!("{}", v.iter().map(|s| format!("{}:{:+.1},{:+.1}", s.0, s.1, s.2)).collect::<Vec<_>>().join(" "));
                }
                if json {
                    println!("{}", serde_json::to_string(&s.report).unwrap_or_default());
                } else {
                    println!(
                        "{} -> {}\n  frames {} kept {} | detail in common between the two halves of the stack: {:.5} | shift rms {:.2} px | rank {:.1} s, stack {:.1} s",
                        f.display(), target.display(), r.frames, r.kept, r.signal, r.shift_rms_px, r.rank_seconds, r.stack_seconds
                    );
                }
            }
        }
    }
    if failed {
        std::process::exit(1);
    }
}
