//! `bb issue` — issue tracker commands.

mod comment;
mod create;
mod list;
mod view;

use crate::core::{Context, FlagError, RepoId};
use clap::{Args, Subcommand};

/// A `FlagError` explaining that the repository's issue tracker is disabled.
/// Bitbucket returns 404 or 410 (Gone) on `/issues` when the feature is off.
pub(super) fn tracker_disabled(repo: &RepoId) -> FlagError {
    FlagError::new(format!(
        "issue tracker is not enabled for {}/{}\n\
         enable it in the repository settings, or this repo may track issues elsewhere (e.g. Jira)",
        repo.workspace(),
        repo.slug()
    ))
}

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
