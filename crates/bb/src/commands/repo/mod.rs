//! `bb repo` — repository commands.

mod clone;
mod create;
mod list;
mod view;

use crate::core::Context;
use clap::{Args, Subcommand};

#[derive(Args, Debug)]
pub struct RepoArgs {
    #[command(subcommand)]
    command: RepoCommands,
}

#[derive(Subcommand, Debug)]
enum RepoCommands {
    /// View a repository
    View(view::ViewArgs),
    /// Create a repository
    Create(create::CreateArgs),
    /// Clone a repository
    Clone(clone::CloneArgs),
    /// List repositories in a workspace
    List(list::ListArgs),
}

/// Dispatch `bb repo <sub>`.
///
/// # Errors
/// Propagates the sub-command's error.
pub fn run(ctx: &Context, args: RepoArgs) -> anyhow::Result<()> {
    match args.command {
        RepoCommands::View(a) => view::run(ctx, a),
        RepoCommands::Create(a) => create::run(ctx, a),
        RepoCommands::Clone(a) => clone::run(ctx, a),
        RepoCommands::List(a) => list::run(ctx, a),
    }
}
