//! `bb issue` — issue tracker commands.

mod comment;
mod create;
mod list;
mod view;

use bb_core::Context;
use clap::{Args, Subcommand};

#[derive(Args, Debug)]
pub struct IssueArgs {
    #[command(subcommand)]
    command: IssueCommands,
}

#[derive(Subcommand, Debug)]
enum IssueCommands {
    /// List issues
    List(list::ListArgs),
    /// View an issue
    View(view::ViewArgs),
    /// Create an issue
    Create(create::CreateArgs),
    /// Comment on an issue
    Comment(comment::CommentArgs),
}

/// Dispatch `bb issue <sub>`.
///
/// # Errors
/// Propagates the sub-command's error.
pub fn run(ctx: &Context, args: IssueArgs) -> anyhow::Result<()> {
    match args.command {
        IssueCommands::List(a) => list::run(ctx, a),
        IssueCommands::View(a) => view::run(ctx, a),
        IssueCommands::Create(a) => create::run(ctx, a),
        IssueCommands::Comment(a) => comment::run(ctx, a),
    }
}
