//! Build script for `rudyd`.
//!
//! Responsibilities:
//!
//! 1. **Embed the `link/` SPA.** `rust-embed` wants a directory to exist at
//!    compile time; if `../../link/dist/` is missing (fresh clone, CI without
//!    a frontend build step) we synthesize a stub so the binary still compiles.
//!    Set `RUDYD_NO_EMBED=1` to force the stub even when `link/dist/` exists
//!    — useful when running against Vite dev server at :5173.
//!
//! 2. **Re-run on changes** to the SPA build or this script itself.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=RUDYD_NO_EMBED");

    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let static_dir = crate_dir.join("static");
    let link_dist = crate_dir.join("../../link/dist").canonicalize().ok();
    let no_embed = env::var("RUDYD_NO_EMBED").is_ok_and(|v| v == "1");

    // Always rebuild if link/dist changes.
    if let Some(ref dist) = link_dist {
        println!("cargo:rerun-if-changed={}", dist.display());
    }

    // Blow away whatever was there; the copy below is idempotent.
    let _ = fs::remove_dir_all(&static_dir);
    fs::create_dir_all(&static_dir).expect("create static/");

    match (link_dist, no_embed) {
        (Some(dist), false) if dist.is_dir() => {
            copy_dir_all(&dist, &static_dir).expect("copy link/dist -> static/");
            println!("cargo:warning=rudyd: embedded link/dist/");
        }
        _ => {
            let stub = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>rudyd (no SPA embedded)</title></head>
<body style="font-family:system-ui;background:#0b0f14;color:#d7dce1;padding:2rem;">
<h1>rudyd is running.</h1>
<p>The <code>link/</code> SPA is not embedded in this build.</p>
<p>During development, run <code>npm run dev</code> inside <code>link/</code>
and point your browser at <code>http://localhost:5173</code>.</p>
<p>For a production build, run <code>npm run build</code> in <code>link/</code>
and rebuild <code>rudyd</code>.</p>
</body></html>"#;
            fs::write(static_dir.join("index.html"), stub).unwrap();
            println!("cargo:warning=rudyd: using stub SPA (set RUDYD_NO_EMBED=0 and build link/ to embed)");
        }
    }
}

fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry?;
        let rel = entry.path().strip_prefix(src).unwrap();
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}
