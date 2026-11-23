//! The file manager's folder view: list any folder of the storage drive (recordings, stacks, logs) with sizes and what each thing
//! is, and download a whole folder as one tar stream. Reading only; deleting stays in `files.rs`.
//!
//! Why tar and not zip: recordings and stacks are already compressed or incompressible, tar needs no checksums, so it can be
//! written while the files are read (no temp copy on the Pi's drive), the size is known up front (the browser shows progress)
//! and macOS, Windows and Linux open it without extra tools.

use crate::api::AppState;
use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use tokio::io::AsyncReadExt;

/// Entries counted below a folder before the size is reported as "at least" (a logs folder can hold tens of thousands of pictures).
const WALK_CAP: usize = 200_000;

#[derive(Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    path: String,
}

fn rel_ok(p: &str) -> bool {
    Path::new(p).components().all(|c| matches!(c, Component::Normal(_)))
}

fn mtime(md: &std::fs::Metadata) -> Option<u64> {
    md.modified().ok().and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs())
}

/// Things the user never needs to see: the "complete" markers, half-written files and the thumbnail cache.
fn hidden(name: &str) -> bool {
    name.ends_with(".ready") || name.ends_with(".tmp") || name.ends_with(".png.tmp") || name == ".thumbs" || name.starts_with('.')
}

/// (bytes, files, capped) below a folder.
fn walk(p: &Path, budget: &mut usize) -> (u64, usize, bool) {
    let (mut bytes, mut files, mut capped) = (0, 0, false);
    let Ok(rd) = std::fs::read_dir(p) else { return (0, 0, false) };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if hidden(&name) {
            continue;
        }
        if *budget == 0 {
            return (bytes, files, true);
        }
        *budget -= 1;
        let Ok(md) = e.metadata() else { continue };
        if md.is_dir() {
            let (b, f, c) = walk(&e.path(), budget);
            bytes += b;
            files += f;
            capped |= c;
        } else {
            bytes += md.len();
            files += 1;
        }
    }
    (bytes, files, capped)
}

/// What the stacker has done: recording → stack and stack → recording, from `stacks/manifest.json` (paths relative to the drive).
struct StackLinks {
    by_source: HashMap<String, Value>,
    by_output: HashMap<String, Value>,
}

fn stack_links(root: &Path) -> StackLinks {
    let mut l = StackLinks { by_source: HashMap::new(), by_output: HashMap::new() };
    let Ok(text) = std::fs::read_to_string(root.join("stacks/manifest.json")) else { return l };
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(&text) else { return l };
    for m in items {
        let (Some(src), Some(out)) = (m["source"].as_str(), m["output"].as_str()) else { continue };
        let out = format!("stacks/{out}");
        let info = json!({ "frames": m["frames"], "kept": m["kept"], "seconds": m["seconds"], "dark": m["dark_applied"], "flat": m["flat_applied"] });
        l.by_source.insert(src.to_string(), json!({ "output": out, "info": info }));
        l.by_output.insert(out, json!({ "source": src, "info": info }));
    }
    l
}

fn classify(rel: &str, name: &str, is_dir: bool, depth: usize) -> &'static str {
    if is_dir {
        return match () {
            _ if depth == 0 && name == "stacks" => "stacks",
            _ if depth == 0 && name == "logs" => "logs",
            _ if name.starts_with("hdr_") => "hdr",
            _ if name.starts_with("prominence_") => "prominence",
            _ if name.starts_with("lane-") => "lane",
            _ if depth == 0 => "day",
            _ => "folder",
        };
    }
    let lower = name.to_lowercase();
    match lower.rsplit('.').next().unwrap_or("") {
        "ser" => "recording",
        "fits" | "fit" if rel.starts_with("stacks/") => "stack",
        "fits" | "fit" => "fits",
        "png" | "jpg" | "jpeg" => "image",
        "json" | "jsonl" | "log" | "txt" => "notes",
        _ => "file",
    }
}

fn list(root: &Path, rel: &str) -> Result<Value, String> {
    if !rel_ok(rel) {
        return Err("invalid path".into());
    }
    let dir = if rel.is_empty() { root.to_path_buf() } else { root.join(rel) };
    let rd = std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", if rel.is_empty() { "the drive" } else { rel }))?;
    let depth = if rel.is_empty() { 0 } else { rel.split('/').count() };
    let links = stack_links(root);
    let raw: Vec<(String, std::fs::Metadata, PathBuf)> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            (!hidden(&name)).then(|| e.metadata().ok().map(|m| (name, m, e.path()))).flatten()
        })
        .collect();
    let names: Vec<&str> = raw.iter().map(|(n, ..)| n.as_str()).collect();
    let mut entries = vec![];
    let mut budget = WALK_CAP;
    for (name, md, path) in &raw {
        // a notes file that belongs to a recording travels with it
        if let Some(stem) = name.strip_suffix(".json") {
            if names.iter().any(|o| *o != name && o.starts_with(stem) && o[stem.len()..].starts_with(|c: char| c == '.' || c == '_')) {
                continue;
            }
        }
        let rpath = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
        let is_dir = md.is_dir();
        let kind = classify(&rpath, name, is_dir, depth);
        let (size, files, capped) = if is_dir { walk(path, &mut budget) } else { (md.len(), 1, false) };
        let mut e = json!({ "name": name, "path": rpath, "dir": is_dir, "type": kind, "size": size, "files": files, "capped": capped, "modified": mtime(md) });
        if let Some(l) = links.by_source.get(&rpath) {
            e["stack"] = l.clone();
        }
        if let Some(l) = links.by_output.get(&rpath) {
            e["from"] = l.clone();
        }
        entries.push(e);
    }
    Ok(json!({ "path": rel, "entries": entries }))
}

/// `GET /api/files/list?path=<folder>`: one folder of the drive ("" is the top).
pub async fn files_list(State(st): State<AppState>, Query(q): Query<ListQuery>) -> Response {
    let root = st.storage.dir();
    match tokio::task::spawn_blocking(move || list(&root, q.path.trim_matches('/'))).await {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    }
}

// ---- tar ----

fn octal(buf: &mut [u8], v: u64) {
    let s = format!("{:0width$o}\0", v, width = buf.len() - 1);
    buf.copy_from_slice(s.as_bytes());
}

/// A ustar header. Long names use the prefix field (up to 155 + 100 bytes); a name that cannot be split is an error, never a silent cut.
fn header_block(name: &str, size: u64, mtime: u64, dir: bool) -> Result<[u8; 512], String> {
    let name = if dir { format!("{name}/") } else { name.to_string() };
    let (prefix, base) = if name.len() <= 100 {
        (String::new(), name.clone())
    } else {
        let cut = name.char_indices().filter(|(i, c)| *c == '/' && *i <= 155 && name.len() - i - 1 <= 100 && *i + 1 < name.len()).map(|(i, _)| i).last();
        match cut {
            Some(i) => (name[..i].to_string(), name[i + 1..].to_string()),
            None => return Err(format!("the name is too long for a tar file: {name}")),
        }
    };
    let mut h = [0u8; 512];
    h[..base.len()].copy_from_slice(base.as_bytes());
    octal(&mut h[100..108], if dir { 0o755 } else { 0o644 });
    octal(&mut h[108..116], 0);
    octal(&mut h[116..124], 0);
    octal(&mut h[124..136], if dir { 0 } else { size });
    octal(&mut h[136..148], mtime);
    h[148..156].copy_from_slice(b"        ");
    h[156] = if dir { b'5' } else { b'0' };
    h[257..263].copy_from_slice(b"ustar\0");
    h[263..265].copy_from_slice(b"00");
    h[345..345 + prefix.len()].copy_from_slice(prefix.as_bytes());
    let sum: u32 = h.iter().map(|b| *b as u32).sum();
    let s = format!("{:06o}\0 ", sum);
    h[148..156].copy_from_slice(s.as_bytes());
    Ok(h)
}

struct Item {
    arc: String,
    path: PathBuf,
    size: u64,
    mtime: u64,
    dir: bool,
}

fn collect(src: &Path, arc: &str, out: &mut Vec<Item>) -> Result<(), String> {
    let md = std::fs::metadata(src).map_err(|e| format!("{}: {e}", src.display()))?;
    if md.is_dir() {
        out.push(Item { arc: arc.to_string(), path: src.to_path_buf(), size: 0, mtime: mtime(&md).unwrap_or(0), dir: true });
        let mut kids: Vec<_> = std::fs::read_dir(src).map_err(|e| e.to_string())?.flatten().collect();
        kids.sort_by_key(|k| k.file_name());
        for k in kids {
            let n = k.file_name().to_string_lossy().to_string();
            if !hidden(&n) {
                collect(&k.path(), &format!("{arc}/{n}"), out)?;
            }
        }
    } else {
        out.push(Item { arc: arc.to_string(), path: src.to_path_buf(), size: md.len(), mtime: mtime(&md).unwrap_or(0), dir: false });
    }
    Ok(())
}

fn padded(n: u64) -> u64 {
    n.div_ceil(512) * 512
}

/// `GET /api/files/archive?path=<folder or file>`: the folder as one .tar download, streamed (nothing is copied on the drive).
pub async fn files_archive(State(st): State<AppState>, Query(q): Query<ListQuery>) -> Response {
    let rel = q.path.trim_matches('/').to_string();
    let bad = |c: StatusCode, e: String| (c, Json(json!({ "error": e }))).into_response();
    if rel.is_empty() || !rel_ok(&rel) {
        return bad(StatusCode::BAD_REQUEST, "choose a folder to download".into());
    }
    // files being written change size under a running tar: refuse, except in stacks/ where the stacker renames finished files into place
    if !rel.starts_with("stacks") && st.engine.status.borrow().capture.state != "idle" {
        return bad(StatusCode::CONFLICT, "a capture is running or still writing: wait until it finishes".into());
    }
    let root = st.storage.dir();
    let src = root.join(&rel);
    let top = rel.rsplit('/').next().unwrap_or(&rel).to_string();
    let items = match tokio::task::spawn_blocking({
        let (src, top) = (src.clone(), top.clone());
        move || {
            let mut v = vec![];
            collect(&src, &top, &mut v)?;
            for i in &v {
                header_block(&i.arc, i.size, i.mtime, i.dir)?; // every name must fit, before the first byte is sent
            }
            Ok::<_, String>(v)
        }
    })
    .await
    {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => return bad(StatusCode::BAD_REQUEST, e),
        Err(e) => return bad(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let length: u64 = items.iter().map(|i| 512 + padded(i.size)).sum::<u64>() + 1024;
    // state: next item, open file with bytes left, pending zero padding, finished
    struct S {
        items: std::vec::IntoIter<Item>,
        cur: Option<(tokio::fs::File, u64, String, u64)>,
        pad: u64,
        end: bool,
    }
    let state = S { items: items.into_iter(), cur: None, pad: 0, end: false };
    let stream = futures_util::stream::unfold(state, |mut s| async move {
        if s.pad > 0 {
            let p = vec![0u8; s.pad as usize];
            s.pad = 0;
            return Some((Ok::<_, std::io::Error>(p), s));
        }
        if let Some((f, left, name, total)) = s.cur.as_mut() {
            if *left > 0 {
                let mut buf = vec![0u8; (*left).min(256 * 1024) as usize];
                return match f.read(&mut buf).await {
                    Ok(0) => {
                        let e = std::io::Error::new(std::io::ErrorKind::UnexpectedEof, format!("{name} got shorter while it was being downloaded"));
                        s.end = true;
                        s.cur = None;
                        Some((Err(e), s))
                    }
                    Ok(n) => {
                        buf.truncate(n);
                        *left -= n as u64;
                        if *left == 0 {
                            s.pad = padded(*total) - *total;
                        }
                        Some((Ok(buf), s))
                    }
                    Err(e) => {
                        s.end = true;
                        s.cur = None;
                        Some((Err(e), s))
                    }
                };
            }
            s.cur = None;
        }
        if s.end {
            return None;
        }
        match s.items.next() {
            Some(i) => {
                let h = header_block(&i.arc, i.size, i.mtime, i.dir).ok()?;
                if !i.dir && i.size > 0 {
                    match tokio::fs::File::open(&i.path).await {
                        Ok(f) => s.cur = Some((f, i.size, i.arc.clone(), i.size)),
                        Err(e) => {
                            s.end = true;
                            return Some((Err(e), s));
                        }
                    }
                }
                Some((Ok(h.to_vec()), s))
            }
            None => {
                s.end = true;
                Some((Ok(vec![0u8; 1024]), s))
            }
        }
    });
    Response::builder()
        .header(header::CONTENT_TYPE, "application/x-tar")
        .header(header::CONTENT_LENGTH, length)
        .header(header::CONTENT_DISPOSITION, format!("attachment; filename=\"{top}.tar\""))
        .body(Body::from_stream(stream))
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rf-browse-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn lists_stacks_inside_nested_folders_and_links_them_to_their_recordings() {
        let d = tmp("list");
        std::fs::create_dir_all(d.join("2026-11-22")).unwrap();
        std::fs::create_dir_all(d.join("stacks/2026-11-22/prominence_1/lane-a")).unwrap();
        std::fs::write(d.join("2026-11-22/burst_1.ser"), vec![0u8; 100]).unwrap();
        std::fs::write(d.join("2026-11-22/burst_1.json"), b"{}").unwrap();
        std::fs::write(d.join("2026-11-22/burst_1.ser.ready"), b"").unwrap();
        std::fs::write(d.join("stacks/2026-11-22/stack_burst_1.fits"), vec![0u8; 50]).unwrap();
        std::fs::write(d.join("stacks/2026-11-22/prominence_1/lane-a/stack_a_c001.fits"), vec![0u8; 20]).unwrap();
        std::fs::create_dir_all(d.join("stacks/.thumbs")).unwrap();
        std::fs::write(
            d.join("stacks/manifest.json"),
            br#"[{"source":"2026-11-22/burst_1.ser","output":"2026-11-22/stack_burst_1.fits","frames":100,"kept":25,"seconds":8.0,"dark_applied":false,"flat_applied":false}]"#,
        )
        .unwrap();
        let top = list(&d, "").unwrap();
        let names: Vec<_> = top["entries"].as_array().unwrap().iter().map(|e| (e["name"].as_str().unwrap().to_string(), e["type"].as_str().unwrap().to_string())).collect();
        assert!(names.contains(&("stacks".into(), "stacks".into())) && names.contains(&("2026-11-22".into(), "day".into())), "{names:?}");
        let day = list(&d, "2026-11-22").unwrap();
        let e = day["entries"].as_array().unwrap();
        assert_eq!(e.len(), 1, "notes and the ready marker travel with the recording: {e:?}");
        assert_eq!(e[0]["stack"]["output"], "stacks/2026-11-22/stack_burst_1.fits");
        let st = list(&d, "stacks/2026-11-22").unwrap();
        let f = st["entries"].as_array().unwrap().iter().find(|e| e["name"] == "stack_burst_1.fits").unwrap();
        assert_eq!((f["type"].as_str(), f["from"]["source"].as_str()), (Some("stack"), Some("2026-11-22/burst_1.ser")));
        let folder = st["entries"].as_array().unwrap().iter().find(|e| e["name"] == "prominence_1").unwrap();
        assert_eq!((folder["type"].as_str(), folder["files"].as_u64(), folder["size"].as_u64()), (Some("prominence"), Some(1), Some(20)));
        assert!(list(&d, "../x").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn tar_headers_are_valid_and_long_names_use_the_prefix_field() {
        let h = header_block("a/b.fits", 1000, 5, false).unwrap();
        assert_eq!(&h[257..262], b"ustar");
        // checksum: the sum of all bytes with the checksum field as spaces
        let mut z = h;
        z[148..156].copy_from_slice(b"        ");
        let sum: u32 = z.iter().map(|b| *b as u32).sum();
        assert_eq!(u32::from_str_radix(std::str::from_utf8(&h[148..154]).unwrap(), 8).unwrap(), sum);
        let long = format!("{}/{}/{}", "d".repeat(60), "e".repeat(60), "f".repeat(40));
        let h = header_block(&long, 1, 0, false).unwrap();
        assert_eq!(&h[345..345 + 121], format!("{}/{}", "d".repeat(60), "e".repeat(60)).as_bytes());
        assert!(header_block(&"x".repeat(300), 1, 0, false).is_err());
    }

    #[test]
    fn a_tar_made_here_is_read_back_by_the_system_tar() {
        // end to end through the same code the endpoint uses
        let d = tmp("tar");
        std::fs::create_dir_all(d.join("day/sub")).unwrap();
        std::fs::write(d.join("day/a.bin"), vec![7u8; 1000]).unwrap();
        std::fs::write(d.join("day/sub/b.bin"), vec![9u8; 512]).unwrap();
        let mut items = vec![];
        collect(&d.join("day"), "day", &mut items).unwrap();
        let mut out = vec![];
        for i in &items {
            out.extend_from_slice(&header_block(&i.arc, i.size, i.mtime, i.dir).unwrap());
            if !i.dir {
                let b = std::fs::read(&i.path).unwrap();
                out.extend_from_slice(&b);
                out.extend(std::iter::repeat(0u8).take((padded(i.size) - i.size) as usize));
            }
        }
        out.extend(std::iter::repeat(0u8).take(1024));
        assert_eq!(out.len() as u64, items.iter().map(|i| 512 + padded(i.size)).sum::<u64>() + 1024);
        let t = d.join("x.tar");
        std::fs::write(&t, &out).unwrap();
        let x = d.join("out");
        std::fs::create_dir_all(&x).unwrap();
        let st = std::process::Command::new("tar").arg("-xf").arg(&t).arg("-C").arg(&x).status();
        if let Ok(st) = st {
            assert!(st.success());
            assert_eq!(std::fs::read(x.join("day/a.bin")).unwrap(), vec![7u8; 1000]);
            assert_eq!(std::fs::read(x.join("day/sub/b.bin")).unwrap(), vec![9u8; 512]);
        }
        let _ = std::fs::remove_dir_all(&d);
    }
}
