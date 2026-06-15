//! Capture the git short SHA and commit date at build time for `bb --version`.

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let sha = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
    let date = git(&["log", "-1", "--format=%cd", "--date=short"])
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=BB_BUILD_SHA={sha}");
    println!("cargo:rustc-env=BB_BUILD_DATE={date}");
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
