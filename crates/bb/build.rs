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
    // Ask git itself where HEAD, the current branch ref, and packed-refs live.
    // `git rev-parse --git-path` resolves these for ordinary checkouts, linked
    // worktrees, AND submodules (where `.git` is a file), so we never
    // reconstruct `.git` paths by hand. If git isn't available the calls return
    // `None` and the rerun triggers are simply skipped.
    if let Some(path) = git(&["rev-parse", "--git-path", "HEAD"]) {
        println!("cargo:rerun-if-changed={path}");
    }
    if let Some(path) = git(&["rev-parse", "--git-path", "packed-refs"]) {
        println!("cargo:rerun-if-changed={path}");
    }
    // Watch the loose ref HEAD points at, so a *commit* on the current branch
    // (not just a checkout) refreshes the embedded SHA.
    if let Some(reference) = git(&["symbolic-ref", "-q", "HEAD"]) {
        if let Some(path) = git(&["rev-parse", "--git-path", &reference]) {
            println!("cargo:rerun-if-changed={path}");
        }
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
