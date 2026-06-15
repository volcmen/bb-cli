//! `bb auth` — authentication commands.

mod login;
mod status;

use bb_core::Context;
use clap::{Args, Subcommand};

#[derive(Args, Debug)]
pub struct AuthArgs {
    #[command(subcommand)]
    command: AuthCommands,
}

#[derive(Subcommand, Debug)]
enum AuthCommands {
    /// Log in to a Bitbucket host
    Login(login::LoginArgs),
    /// View authentication status
    Status(status::StatusArgs),
}

/// Dispatch `bb auth <sub>`.
///
/// # Errors
/// Propagates the sub-command's error.
pub fn run(ctx: &Context, args: AuthArgs) -> anyhow::Result<()> {
    match args.command {
        AuthCommands::Login(a) => login::run(ctx, a),
        AuthCommands::Status(a) => status::run(ctx, a),
    }
}
