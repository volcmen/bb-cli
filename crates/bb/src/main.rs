//! `bb` — entry point and exit-code mapping (mirrors `gh`'s `mainRun`/`printError`).

// Former workspace crates, now modules of the single published `bb-cli` crate.
mod api;
mod config;
mod core;
mod git;

mod auth;
mod browser;
mod cli;
mod commands;
mod factory;
mod output;
mod prompt;
mod refresh;
mod render;
#[cfg(test)]
mod testsupport;

use std::process::ExitCode;

use crate::core::{ApiError, AuthError, CancelError, ExitCode as Bb, FlagError, SilentError};

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();

    // Expand a leading user alias before clap sees the args (best-effort: a
    // config load failure just means no aliases).
    let aliases = config::load()
        .ok()
        .map(|c| crate::commands::alias::load_aliases(c.as_ref()))
        .unwrap_or_default();
    match crate::commands::alias::expand(&argv, &cli::builtin_names(), &aliases) {
        crate::commands::alias::Expanded::Shell(line) => run_shell_alias(&line),
        crate::commands::alias::Expanded::Clap(args) => {
            // `clap` auto-handles `--version`, `--help`, and parse errors (exit 2).
            let cli = cli::parse_from(args);
            match cli::dispatch(cli) {
                Ok(()) => ExitCode::from(Bb::Ok.as_u8()),
                Err(err) => ExitCode::from(classify(&err).as_u8()),
            }
        }
    }
}

/// Execute a `!`-shell alias through the platform shell, propagating its exit code.
fn run_shell_alias(line: &str) -> ExitCode {
    #[cfg(windows)]
    let mut cmd = {
        let mut c = std::process::Command::new("cmd");
        c.arg("/C");
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = std::process::Command::new("sh");
        c.arg("-c");
        c
    };
    match cmd.arg(line).status() {
        Ok(status) => ExitCode::from(u8::try_from(status.code().unwrap_or(1)).unwrap_or(1)),
        Err(e) => {
            eprintln!("error: failed to run shell alias: {e}");
            ExitCode::from(Bb::Error.as_u8())
        }
    }
}

/// Print the error appropriately and choose the exit code (mirrors `gh`).
fn classify(err: &anyhow::Error) -> Bb {
    if err.is::<SilentError>() {
        return Bb::Error;
    }
    if err.is::<CancelError>() {
        eprintln!();
        return Bb::Cancel;
    }
    if let Some(auth) = err.downcast_ref::<AuthError>() {
        eprintln!("error: {auth}");
        eprintln!(
            "To authenticate, run: bb auth login --hostname {}",
            auth.hostname
        );
        return Bb::Auth;
    }

    eprintln!("error: {err}");
    if err.downcast_ref::<FlagError>().is_some() {
        return Bb::Error;
    }
    if let Some(api) = err.downcast_ref::<ApiError>() {
        if api.is_unauthorized() {
            eprintln!("hint: your credentials may be invalid — run `bb auth login`");
        }
    }
    Bb::Error
}
