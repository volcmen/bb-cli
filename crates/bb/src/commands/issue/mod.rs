//! `bb issue` — issue tracker commands.

mod comment;
mod create;
mod edit;
mod list;
pub(crate) mod query;
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
    /// Edit an issue's title/body/kind/priority/state
    Edit(edit::EditArgs),
    /// Close an issue (state → resolved)
    Close(edit::StateArgs),
    /// Reopen an issue (state → open)
    Reopen(edit::StateArgs),
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
        IssueCommands::Edit(a) => edit::run(ctx, a),
        IssueCommands::Close(a) => edit::run_close(ctx, a),
        IssueCommands::Reopen(a) => edit::run_reopen(ctx, a),
    }
}
