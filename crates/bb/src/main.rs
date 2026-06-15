//! `bb` — entry point and exit-code mapping (mirrors `gh`'s `mainRun`/`printError`).

mod auth;
mod browser;
mod cli;
mod commands;
mod factory;
mod output;
mod prompt;
#[cfg(test)]
mod testsupport;

use std::process::ExitCode;

use bb_core::{ApiError, AuthError, CancelError, ExitCode as Bb, FlagError, SilentError};

fn main() -> ExitCode {
    // `clap` auto-handles `--version`, `--help`, and parse errors (exit 2).
    let cli = cli::parse();
    match cli::dispatch(cli) {
        Ok(()) => ExitCode::from(Bb::Ok.as_u8()),
        Err(err) => ExitCode::from(classify(&err).as_u8()),
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
