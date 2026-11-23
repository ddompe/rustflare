//! Stacking everything in a session folder: every SER (lanes included) becomes a FITS still under `stacks/`, mirroring the
//! folder structure, with a manifest. A recording whose stack already exists (and is newer) is skipped, so a run can be
//! repeated, or a watcher can call it again as files arrive.

use crate::calib::{CalSet, Settings};
use crate::output::write_fits;
use crate::stack::Options;
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
    #[serde(default)]
    pub phases: Vec<(String, f64)>,
    /// Where the Sun's disc was found in the stack (prominence lanes), and whether a black disc was drawn there.
    #[serde(default)]
    pub disc: Option<crate::disc::DiscInfo>,
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

/// Stack one recording: write its FITS under `out` (mirroring the folder structure below `root`) and return the manifest entry.
pub fn stack_one(root: &Path, out: &Path, src: &Path, opts: &Options, cals: &CalSet, float: bool) -> Result<Entry, String> {
    let ser = crate::ser::Ser::open(src).map_err(|e| e.to_string())?;
    let track = crate::disc::track_from_sidecar(src);
    let result = stack_source(root, out, src, &ser, Settings::from_sidecar(src), track.as_deref(), opts, cals, float).map(|(e, _)| e);
    crate::source::FrameSource::release(&ser);
    result
}

/// Stack frames from any source (a SER file, a burst in the frame ring). `name` is where the recording is or would be: it names the
/// output and goes into the FITS header and the manifest. Returns the entry and the ranking score of every frame (higher is better),
/// which tells a recorder which frames are worth keeping.
pub fn stack_source(root: &Path, out: &Path, name: &Path, src: &dyn crate::source::FrameSource, settings: Settings, disc_track: Option<&[crate::disc::Pos]>, opts: &Options, cals: &CalSet, float: bool) -> Result<(Entry, Vec<f64>), String> {
    let target = output_path(root, out, name);
    let rel = name.strip_prefix(root).unwrap_or(name).to_string_lossy().to_string();
    let (cal, mut notes) = cals.for_settings(settings);
    let t0 = std::time::Instant::now();
    let mut stacked = crate::stack::stack_frames(src, opts, &cal)?;
    // prominence lanes: find the disc in the stack and, if asked, black it out (the frames themselves are raw)
    if let Some(track) = disc_track.filter(|t| !t.is_empty()) {
        if track.len() != src.frames() {
            notes.push(format!("the recorded disc positions ({}) do not match the frames ({}): no disc drawn", track.len(), src.frames()));
        } else {
            match crate::disc::locate(&stacked, track) {
                Ok(mut d) => {
                    if d.how != "fitted" {
                        notes.push("the disc's edge could not be found in the stack: its recorded position was used".into());
                    }
                    if opts.disc_mask {
                        crate::disc::blank(&mut stacked, &mut d, opts.disc_scale);
                    }
                    stacked.disc = Some(d);
                }
                Err(e) => notes.push(format!("no disc drawn: {e}")),
            }
        }
    }
    write_fits(&target, &stacked, name, opts, float, settings).map_err(|e| format!("{}: {e}", target.display()))?;
    let r = &stacked.report;
    let entry = Entry {
        source: rel,
        output: target.strip_prefix(out).unwrap_or(&target).to_string_lossy().to_string(),
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
        phases: r.phases.clone(),
        disc: stacked.disc.clone(),
    };
    Ok((entry, stacked.report.score.clone()))
}

/// Add `entry` to the manifest in `out` (replacing an older entry for the same source), written atomically.
pub fn record(out: &Path, entry: &Entry) -> Result<(), String> {
    let path = out.join("manifest.json");
    let mut manifest: Vec<Entry> = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    manifest.retain(|e| e.source != entry.source);
    manifest.push(entry.clone());
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&manifest).unwrap_or_default()).and_then(|_| std::fs::rename(&tmp, &path)).map_err(|e| e.to_string())
}

/// Whether `src` has a stack at least as new as itself.
pub fn is_stacked(root: &Path, out: &Path, src: &Path) -> bool {
    let target = output_path(root, out, src);
    target.exists() && newer(&target, src)
}

/// Stack every recording under `root` into `out` (default `root/stacks`). Returns the entries of this run.
pub fn run(root: &Path, out: Option<&Path>, opts: &Options, cals: &CalSet, float: bool, force: bool, log: &dyn Fn(&str)) -> Result<Vec<Entry>, String> {
    let out = out.map(|p| p.to_path_buf()).unwrap_or_else(|| root.join("stacks"));
    let mut done = Vec::new();
    for src in find_recordings(root) {
        let rel = src.strip_prefix(root).unwrap_or(&src).to_string_lossy().to_string();
        if !force && is_stacked(root, &out, &src) {
            log(&format!("skip {rel} (already stacked)"));
            continue;
        }
        match stack_one(root, &out, &src, opts, cals, float) {
            Ok(entry) => {
                log(&format!("{rel}: {} of {} frames, shift rms {:.2} px, {:.1} s", entry.kept, entry.frames, entry.shift_rms_px, entry.seconds));
                record(&out, &entry)?; // after every recording, so a stopped run keeps what it did
                done.push(entry);
            }
            Err(e) => log(&format!("{rel}: {e}")),
        }
    }
    Ok(done)
}
