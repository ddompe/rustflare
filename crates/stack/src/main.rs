//! `rustflare-stack stack <file.ser>...` and `rustflare-stack session <folder>`: rank, align and stack recordings into FITS stills.

use rustflare_stack::{calib::{CalSet, Settings}, output::write_fits, session, stack::{stack_ser, Options, Rank}};
use std::path::PathBuf;

const USAGE: &str = "usage: rustflare-stack stack <file.ser>... [options]
       rustflare-stack session <folder> [options]       every recording under the folder, into <folder>/stacks/
options:
  --keep PERCENT     frames to keep, the best first (default 25)
  --out DIR          where the stacks go (stack: next to each recording in stacks/; session: <folder>/stacks)
  --flat flat.ser    raw flat-field recording; the master is built from it
  --dark dark.ser    raw dark recording (repeatable); one is chosen per recording by exposure and gain from the sidecars
  --clip SIGMA       sigma-clip the mean (default off, 2.5 is a good start); works on the global-shift stack, so it turns multi-point off
  --refine PERCENT   align a second time against the mean of the best PERCENT of the frames (default 10, 0 = off)
  --global           global alignment only (default: multi-point, a shift and a frame choice per alignment point)
  --ap-half N  --search N  --register PERCENT  --min-structure F   multi-point tuning (24, 14, 50, 0.05)
  --no-align  --rank reference|laplacian|order|worst  --u16 (unsigned 16-bit FITS; default is 32-bit float)
  --threads N  --bin N  --whiten 0..1  --integer-shifts  --force (session: redo finished ones)  --json  --shifts";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first().cloned().unwrap_or_default();
    if (mode != "stack" && mode != "session") || args.len() < 2 {
        eprintln!("{USAGE}");
        std::process::exit(2);
    }
    let (mut inputs, mut out, mut flat, mut darks) = (Vec::<PathBuf>::new(), None::<PathBuf>, None::<PathBuf>, Vec::<PathBuf>::new());
    let (mut opts, mut float, mut json, mut print_shifts, mut force) = (Options::default(), true, false, false, false);
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
            "--dark" => darks.push(val(&mut i).into()),
            "--clip" => opts.clip = val(&mut i).parse().unwrap_or(0.0),
            "--refine" => opts.refine = val(&mut i).parse::<f64>().unwrap_or(10.0) / 100.0,
            "--no-align" => opts.align = false,
            "--global" => opts.multipoint = false,
            "--ap-half" => opts.ap_half = val(&mut i).parse().unwrap_or(24),
            "--search" => opts.search = val(&mut i).parse().unwrap_or(14),
            "--register" => opts.register = val(&mut i).parse::<f64>().unwrap_or(50.0) / 100.0,
            "--min-structure" => opts.min_structure = val(&mut i).parse().unwrap_or(0.05),
            "--u16" => float = false,
            "--float" => float = true,
            "--force" => force = true,
            "--json" => json = true,
            "--shifts" => print_shifts = true,
            "--integer-shifts" => opts.integer_shifts = true,
            "--rank" => opts.rank = match val(&mut i).as_str() { "laplacian" => Rank::Laplacian, "order" => Rank::Order, "worst" => Rank::Worst, _ => Rank::Reference },
            "--threads" => opts.threads = val(&mut i).parse().unwrap_or(2),
            "--bin" => opts.bin = val(&mut i).parse().unwrap_or(2),
            "--whiten" => opts.whiten = val(&mut i).parse().unwrap_or(0.7),
            f if !f.starts_with("--") => inputs.push(f.into()),
            other => {
                eprintln!("unknown option {other}\n{USAGE}");
                std::process::exit(2);
            }
        }
        i += 1;
    }
    let cals = match CalSet::load(&darks, flat.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("calibration: {e}");
            std::process::exit(1);
        }
    };
    if mode == "session" {
        let Some(root) = inputs.first() else { eprintln!("{USAGE}"); std::process::exit(2) };
        match session::run(root, out.as_deref(), &opts, &cals, float, force, &|m| println!("{m}")) {
            Ok(done) => println!("{} recordings stacked", done.len()),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        return;
    }
    let mut failed = false;
    for f in &inputs {
        let dir = out.clone().unwrap_or_else(|| f.parent().unwrap_or(std::path::Path::new(".")).join("stacks"));
        let settings = Settings::from_sidecar(f);
        let (cal, notes) = cals.for_settings(settings);
        for n in notes {
            eprintln!("{}: {n}", f.display());
        }
        match stack_ser(f, &opts, &cal) {
            Err(e) => {
                eprintln!("{}: {e}", f.display());
                failed = true;
            }
            Ok(s) => {
                let stem = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "stack".into());
                let target = dir.join(format!("stack_{stem}.fits"));
                if let Err(e) = write_fits(&target, &s, f, &opts, float, settings) {
                    eprintln!("{}: {e}", target.display());
                    failed = true;
                    continue;
                }
                let r = &s.report;
                if print_shifts {
                    let mut v = r.shifts.clone();
                    v.sort_by_key(|s| s.0);
                    println!("{}", v.iter().map(|s| format!("{}:{:+.1},{:+.1}", s.0, s.1, s.2)).collect::<Vec<_>>().join(" "));
                }
                if json {
                    println!("{}", serde_json::to_string(r).unwrap_or_default());
                } else {
                    println!(
                        "{} -> {}\n  frames {} kept {} | split-half detail {:.5} (global-shift stack {:.5}) | {} points, their shifts differ from the global one by {:.2} px rms ({:.1} s) | global shift rms {:.2} px, second pass moved {:.2} px | clipped {:.2}% | rank {:.1} s, stack {:.1} s",
                        f.display(), target.display(), r.frames, r.kept, r.signal, r.signal_global, r.aps, r.local_shift_rms_px, r.multipoint_seconds, r.shift_rms_px, r.refine_change_px, r.clipped_fraction * 100.0, r.rank_seconds, r.stack_seconds
                    );
                }
            }
        }
    }
    if failed {
        std::process::exit(1);
    }
}
