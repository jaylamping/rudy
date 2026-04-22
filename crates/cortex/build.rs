//! Build script for `cortex`.
//!
//! Responsibilities:
//!
//! 1. **Embed the `link/` SPA.** `rust-embed` wants a directory to exist at
//!    compile time; if `../../link/dist/` is missing (fresh clone, CI without
//!    a frontend build step) we synthesize a stub so the binary still compiles.
//!    Set `CORTEX_NO_EMBED=1` to force the stub even when `link/dist/` exists
//!    — useful when running against Vite dev server at :5173.
//!
//! 2. **Re-run on changes** to the SPA build or this script itself.
//!
//! 3. **Compile-time `CORTEX_*` identity** (`COMMIT_SHA`, `SHORT_SHA`, `BUILT_AT`)
//!    for `GET /api/config` and operator-console build stamps. Set in CI; falls
//!    back to `git` when present and `unknown` when not.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CORTEX_NO_EMBED");
    for k in ["CORTEX_COMMIT_SHA", "CORTEX_SHORT_SHA", "CORTEX_BUILT_AT"] {
        println!("cargo:rerun-if-env-changed={k}");
    }

    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let commit = env::var("CORTEX_COMMIT_SHA")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let mut cmd = Command::new("git");
            cmd.current_dir(&crate_dir).args(["rev-parse", "HEAD"]);
            read_trimmed_ok(cmd)
        })
        .unwrap_or_else(|| "unknown".to_string());

    let short = env::var("CORTEX_SHORT_SHA")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            (commit != "unknown" && commit.len() >= 12).then(|| commit.chars().take(12).collect())
        })
        .or_else(|| {
            let mut cmd = Command::new("git");
            cmd.current_dir(&crate_dir)
                .args(["rev-parse", "--short=12", "HEAD"]);
            read_trimmed_ok(cmd)
        })
        .unwrap_or_else(|| "unknown".to_string());

    // Prefer CI / explicit `CORTEX_BUILT_AT` (aligns with `latest.json`), else
    // the committer date of HEAD (repro + works offline), else "unknown".
    let built_at = env::var("CORTEX_BUILT_AT")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let mut cmd = Command::new("git");
            cmd.current_dir(&crate_dir)
                .args(["log", "-1", "--format=%cI", "HEAD"]);
            read_trimmed_ok(cmd)
        })
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=CORTEX_COMMIT_SHA={commit}");
    println!("cargo:rustc-env=CORTEX_SHORT_SHA={short}");
    println!("cargo:rustc-env=CORTEX_BUILT_AT={built_at}");
    let static_dir = crate_dir.join("static");
    let link_dist = crate_dir.join("../../link/dist").canonicalize().ok();
    let no_embed = env::var("CORTEX_NO_EMBED").is_ok_and(|v| v == "1");

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
        }
        _ => {
            let stub = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>cortex (no SPA embedded)</title></head>
<body style="font-family:system-ui;background:#0b0f14;color:#d7dce1;padding:2rem;">
<h1>cortex is running.</h1>
<p>The <code>link/</code> SPA is not embedded in this build.</p>
<p>During development, run <code>npm run dev</code> inside <code>link/</code>
and point your browser at <code>http://localhost:5173</code>.</p>
<p>For a production build, run <code>npm run build</code> in <code>link/</code>
and rebuild <code>cortex</code>.</p>
</body></html>"#;
            fs::write(static_dir.join("index.html"), stub).unwrap();
        }
    }
}

fn read_trimmed_ok(mut cmd: Command) -> Option<String> {
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?;
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
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
