//! Writing a stacked still: FITS with the headers a stacker or viewer wants.

use crate::calib::Settings;
use crate::fits::{self, Extra};
use crate::stack::{Options, Stacked};
use std::path::Path;

/// "2026-11-22T17:48:55" for seconds since the Unix epoch (UTC).
pub fn iso_utc(t: f64) -> String {
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
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}

pub fn write_fits(target: &Path, s: &Stacked, source: &Path, opts: &Options, float: bool, settings: Settings) -> std::io::Result<()> {
    let r = &s.report;
    let flag = |b: bool| if b { "T" } else { "F" }.to_string();
    let mut extra: Vec<Extra> = vec![
        ("STACKED".into(), r.kept.to_string(), "frames averaged".into()),
        ("NFRAMES".into(), r.frames.to_string(), "frames in the recording".into()),
        ("KEEPFRAC".into(), format!("{:.3}", opts.keep), "fraction kept, best first".into()),
        ("ALIGNED".into(), flag(opts.align), "phase-correlation alignment".into()),
        ("SIGCLIP".into(), format!("{:.1}", opts.clip), "sigma clip (0 = off)".into()),
        ("FLATCORR".into(), flag(r.flat_applied), "master flat applied".into()),
        ("DARKCORR".into(), flag(r.dark_applied), "master dark applied".into()),
        ("ORIGFILE".into(), fits::quoted(&source.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()), "source recording".into()),
    ];
    if let Some(t) = r.first_timestamp {
        extra.push(("DATE-OBS".into(), fits::quoted(&iso_utc(t)), "first frame, UTC".into()));
    }
    if let Some(e) = settings.exposure_us {
        extra.push(("EXPTIME".into(), format!("{:.6}", e / 1e6), "seconds per frame".into()));
    }
    if let Some(g) = settings.gain {
        extra.push(("GAIN".into(), format!("{g}"), "camera gain".into()));
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    fits::write(target, s.width, s.height, &s.data, float, &extra)
}
