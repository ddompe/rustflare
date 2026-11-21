//! With `--features playerone`, make the binary able to find Player One's vendor library at run time.
//!
//! `playerone-sdk-sys` links `libPlayerOneCamera` but ships it as `libPlayerOneCamera.so` while the library's
//! SONAME is `libPlayerOneCamera.so.3` (`.3.dylib` on macOS), and it sets no rpath. So we (1) add an rpath
//! relative to the executable, (2) point the linker at the right per-target folder (the sys crate uses the host
//! OS, which breaks cross-compiling) and (3) copy the library next to the binary under its SONAME.
//! See ADR 0006.

use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var_os("CARGO_FEATURE_PLAYERONE").is_none() {
        return;
    }
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let (subdir, src_name, dst_name, rpaths): (PathBuf, &str, &str, &[&str]) = match (os.as_str(), arch.as_str()) {
        ("linux", "aarch64") => ("linux/arm64".into(), "libPlayerOneCamera.so", "libPlayerOneCamera.so.3", &["$ORIGIN", "$ORIGIN/lib"]),
        ("linux", "x86_64") => ("linux/x64".into(), "libPlayerOneCamera.so", "libPlayerOneCamera.so.3", &["$ORIGIN", "$ORIGIN/lib"]),
        ("macos", _) => ("mac".into(), "libPlayerOneCamera.dylib", "libPlayerOneCamera.3.dylib", &["@executable_path"]),
        _ => {
            println!("cargo:warning=Player One library is not available for {os}/{arch}");
            return;
        }
    };
    for r in rpaths {
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,{r}");
    }

    let home = std::env::var_os("CARGO_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".cargo")));
    let Some(home) = home else {
        println!("cargo:warning=cannot locate CARGO_HOME; copy {dst_name} next to the binary yourself");
        return;
    };
    let Some(src) = find_lib(&home.join("registry/src"), &subdir, src_name) else {
        println!("cargo:warning=playerone-sdk-sys library not found under {}; copy {dst_name} next to the binary yourself", home.display());
        return;
    };
    // playerone-sdk-sys picks its library folder from the *host* OS, which is wrong when cross-compiling
    // (e.g. building for the Pi on a Mac). Add the correct folder for the *target* ourselves.
    if let Some(dir) = src.parent() {
        println!("cargo:rustc-link-search=native={}", dir.display());
    }

    // OUT_DIR is somewhere below <target>/<profile>/build/...; the binary lives in <target>/<profile>.
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let profile_dir = out.ancestors().find(|p| p.file_name().is_some_and(|n| n == "build")).and_then(|b| b.parent());
    match profile_dir {
        Some(dir) => {
            let dst = dir.join(dst_name);
            match std::fs::copy(&src, &dst) {
                Ok(_) => println!("cargo:warning=copied {} to {}", src.display(), dst.display()),
                Err(e) => println!("cargo:warning=could not copy vendor library to {}: {e}", dst.display()),
            }
        }
        None => println!("cargo:warning=cannot locate the target directory; copy {dst_name} next to the binary yourself"),
    }
}

fn find_lib(registry_src: &Path, subdir: &Path, name: &str) -> Option<PathBuf> {
    for index in std::fs::read_dir(registry_src).ok()?.flatten() {
        for krate in std::fs::read_dir(index.path()).ok()?.flatten() {
            if krate.file_name().to_string_lossy().starts_with("playerone-sdk-sys-") {
                let p = krate.path().join("libs").join(subdir).join(name);
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }
    None
}
