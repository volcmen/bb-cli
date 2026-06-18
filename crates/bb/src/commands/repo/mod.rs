//! `bb repo` — repository commands.

mod clone;
mod create;
mod edit;
mod fork;
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
    /// Fork a repository
    Fork(fork::ForkArgs),
    /// Edit repository settings (description, visibility, project)
    Edit(edit::EditArgs),
    /// Rename a repository
    Rename(edit::RenameArgs),
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
        RepoCommands::Fork(a) => fork::run(ctx, a),
        RepoCommands::Edit(a) => edit::run(ctx, a),
        RepoCommands::Rename(a) => edit::run_rename(ctx, a),
        RepoCommands::List(a) => list::run(ctx, a),
    }
}
