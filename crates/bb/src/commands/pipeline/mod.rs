//! `bb pipeline` — CI pipeline commands.

mod list;
mod view;

use crate::core::Context;
use clap::{Args, Subcommand};

#[derive(Args, Debug)]
pub struct PipelineArgs {
    #[command(subcommand)]
    command: PipelineCommands,
}

#[derive(Subcommand, Debug)]
enum PipelineCommands {
    /// List recent pipelines
    List(list::ListArgs),
    /// View a pipeline (by build number)
    View(view::ViewArgs),
}

/// Dispatch `bb pipeline <sub>`.
///
/// # Errors
/// Propagates the sub-command's error.
pub fn run(ctx: &Context, args: PipelineArgs) -> anyhow::Result<()> {
    match args.command {
        PipelineCommands::List(a) => list::run(ctx, a),
        PipelineCommands::View(a) => view::run(ctx, a),
    }
}
