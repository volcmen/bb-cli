//! Capture the git short SHA and commit date at build time for `bb --version`.

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
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
