//! Stacking everything in a session folder: every SER (lanes included) becomes a FITS still under `stacks/`, mirroring the
//! folder structure, with a manifest. A recording whose stack already exists (and is newer) is skipped, so a run can be
//! repeated, or a watcher can call it again as files arrive.

use crate::calib::{CalSet, Settings};
use crate::output::write_fits;
use crate::stack::{stack_ser, Options};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Entry {
    /// Source recording, relative to the session folder.
    pub source: String,
    pub output: String,
    pub frames: usize,
    pub kept: usize,
    pub signal: f64,
    pub shift_rms_px: f64,
    pub refine_change_px: f64,
    pub clipped_fraction: f64,
    pub exposure_us: Option<f64>,
    pub gain: Option<f64>,
    pub dark_applied: bool,
    pub flat_applied: bool,
    pub notes: Vec<String>,
    pub seconds: f64,
}

/// Recordings under `root`, oldest name first. Calibration recordings, `_partN` continuations and the output folder are left out.
pub fn find_recordings(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if p.is_dir() {
                if name != "stacks" && !name.starts_with('.') {
                    walk(&p, out);
                }
            } else if name.ends_with(".ser") && !name.starts_with("dark_") && !name.starts_with("flat_") && !name.contains("_part") && !name.starts_with("._") {
                out.push(p);
            }
        }
    }
    let mut v = Vec::new();
    walk(root, &mut v);
    v.sort();
    v
}

pub fn output_path(root: &Path, out: &Path, src: &Path) -> PathBuf {
    let rel = src.strip_prefix(root).unwrap_or(src);
    let stem = rel.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    out.join(rel.parent().unwrap_or(Path::new(""))).join(format!("stack_{stem}.fits"))
}

fn newer(a: &Path, b: &Path) -> bool {
    match (std::fs::metadata(a).and_then(|m| m.modified()), std::fs::metadata(b).and_then(|m| m.modified())) {
        (Ok(x), Ok(y)) => x >= y,
        _ => false,
    }
}

/// Stack every recording under `root` into `out` (default `root/stacks`). Returns the entries of this run.
pub fn run(root: &Path, out: Option<&Path>, opts: &Options, cals: &CalSet, float: bool, force: bool, log: &dyn Fn(&str)) -> Result<Vec<Entry>, String> {
    let out = out.map(|p| p.to_path_buf()).unwrap_or_else(|| root.join("stacks"));
    let manifest_path = out.join("manifest.json");
    let mut manifest: Vec<Entry> = std::fs::read_to_string(&manifest_path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let mut done = Vec::new();
    for src in find_recordings(root) {
        let target = output_path(root, &out, &src);
        let rel = src.strip_prefix(root).unwrap_or(&src).to_string_lossy().to_string();
        if !force && target.exists() && newer(&target, &src) {
            log(&format!("skip {rel} (already stacked)"));
            continue;
        }
        let settings = Settings::from_sidecar(&src);
        let (cal, notes) = cals.for_settings(settings);
        let t0 = std::time::Instant::now();
        let stacked = match stack_ser(&src, opts, &cal) {
            Ok(s) => s,
            Err(e) => {
                log(&format!("{rel}: {e}"));
                continue;
            }
        };
        write_fits(&target, &stacked, &src, opts, float, settings).map_err(|e| format!("{}: {e}", target.display()))?;
        let r = &stacked.report;
        let entry = Entry {
            source: rel.clone(),
            output: target.strip_prefix(&out).unwrap_or(&target).to_string_lossy().to_string(),
            frames: r.frames,
            kept: r.kept,
            signal: r.signal,
            shift_rms_px: r.shift_rms_px,
            refine_change_px: r.refine_change_px,
            clipped_fraction: r.clipped_fraction,
            exposure_us: settings.exposure_us,
            gain: settings.gain,
            dark_applied: r.dark_applied,
            flat_applied: r.flat_applied,
            notes,
            seconds: t0.elapsed().as_secs_f64(),
        };
        log(&format!("{rel}: {} of {} frames, shift rms {:.2} px, {:.1} s", entry.kept, entry.frames, entry.shift_rms_px, entry.seconds));
        manifest.retain(|e| e.source != entry.source);
        manifest.push(entry.clone());
        done.push(entry);
        std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        // written after every recording, so a stopped run keeps what it did
        let tmp = manifest_path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&manifest).unwrap_or_default()).and_then(|_| std::fs::rename(&tmp, &manifest_path)).map_err(|e| e.to_string())?;
    }
    Ok(done)
}
