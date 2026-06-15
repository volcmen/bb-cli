//! `bb pr` — pull request commands.

mod create;
mod list;

use bb_core::Context;
use clap::{Args, Subcommand};

#[derive(Args, Debug)]
pub struct PrArgs {
    #[command(subcommand)]
    command: PrCommands,
}

#[derive(Subcommand, Debug)]
enum PrCommands {
    /// Create a pull request
    Create(create::CreateArgs),
    /// List pull requests
    List(list::ListArgs),
}

/// Dispatch `bb pr <sub>`.
///
/// # Errors
/// Propagates the sub-command's error.
pub fn run(ctx: &Context, args: PrArgs) -> anyhow::Result<()> {
    match args.command {
        PrCommands::Create(a) => create::run(ctx, a),
        PrCommands::List(a) => list::run(ctx, a),
    }
}
