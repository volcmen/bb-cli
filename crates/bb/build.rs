//! Capture the git short SHA and commit date at build time for `bb --version`.

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // Refresh the embedded SHA/date whenever HEAD or the checked-out branch ref
    // moves. Without this, cargo caches build.rs's output and `bb --version`
    // reports a stale commit after incremental rebuilds.
    rerun_on_git_head();
    let sha = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
    let date = git(&["log", "-1", "--format=%cd", "--date=short"])
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=BB_BUILD_SHA={sha}");
    println!("cargo:rustc-env=BB_BUILD_DATE={date}");

    // Bake an OAuth consumer into the binary if provided at build time (the
    // analog of bkt's ldflags-injected release credentials). Source builds
    // without these set fall back to flags/env/config at runtime.
    for (src, dst) in [
        ("BB_OAUTH_CLIENT_ID", "BB_EMBED_OAUTH_CLIENT_ID"),
        ("BB_OAUTH_CLIENT_SECRET", "BB_EMBED_OAUTH_CLIENT_SECRET"),
    ] {
        println!("cargo:rerun-if-env-changed={src}");
        if let Ok(v) = std::env::var(src) {
            if !v.is_empty() {
                println!("cargo:rustc-env={dst}={v}");
            }
        }
    }
}

/// Tell cargo to re-run this build script when the git HEAD (or the ref it
/// points to) changes, so the embedded commit SHA/date stay current.
fn rerun_on_git_head() {
    let manifest = match std::env::var("CARGO_MANIFEST_DIR") {
        Ok(v) => v,
        Err(_) => return,
    };
    // Workspace root is two levels up from `crates/bb`.
    let git_dir = std::path::Path::new(&manifest).join("../../.git");
    let head = git_dir.join("HEAD");
    if !head.exists() {
        return;
    }
    println!("cargo:rerun-if-changed={}", head.display());

    // Follow a symbolic HEAD ("ref: refs/heads/<branch>") to its loose ref file.
    if let Ok(contents) = std::fs::read_to_string(&head) {
        if let Some(reference) = contents.strip_prefix("ref:").map(str::trim) {
            let ref_path = git_dir.join(reference);
            if ref_path.exists() {
                println!("cargo:rerun-if-changed={}", ref_path.display());
            }
        }
    }
    // Cover the case where the branch ref is packed rather than a loose file.
    let packed = git_dir.join("packed-refs");
    if packed.exists() {
        println!("cargo:rerun-if-changed={}", packed.display());
    }
}

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}
