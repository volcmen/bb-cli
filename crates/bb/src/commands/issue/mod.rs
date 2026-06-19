//! `bb issue` — issue tracker commands.

mod comment;
mod create;
mod edit;
mod list;
pub(crate) mod query;
mod view;

use crate::core::{ApiError, Context, FlagError, RepoId};
use clap::{Args, Subcommand};

/// A `FlagError` explaining that the repository's issue tracker is disabled.
/// Bitbucket returns 410 (Gone) — or a 404 whose body says the repository has
/// no issue tracker — on `/issues` when the feature is off.
pub(super) fn tracker_disabled(repo: &RepoId) -> FlagError {
    FlagError::new(format!(
        "issue tracker is not enabled for {}/{}\n\
         enable it in the repository settings, or this repo may track issues elsewhere (e.g. Jira)",
        repo.workspace(),
        repo.slug()
    ))
}

/// A `FlagError` for a repository that doesn't exist or that the caller can't
/// see — the other thing a 404 on an issue endpoint can mean. Mirrors the
/// phrasing `bb repo view` uses for a missing repo.
pub(super) fn repo_not_found(repo: &RepoId) -> FlagError {
    FlagError::new(format!(
        "repository {}/{} not found (or you don't have access)",
        repo.workspace(),
        repo.slug()
    ))
}

/// Does this error's body say the repository has no issue tracker? Bitbucket
/// returns a 404 with a message like "Repository has no issue tracker." when
/// the feature is disabled, which we must not confuse with a missing repo.
fn is_no_tracker(e: &ApiError) -> bool {
    e.http_message()
        .is_some_and(|m| m.to_ascii_lowercase().contains("no issue tracker"))
}

/// Classify a 404 on an issue endpoint that does **not** name a specific issue
/// (e.g. `GET`/`POST .../issues`): a "no issue tracker" body means the tracker
/// is disabled, anything else means the repo is missing or inaccessible.
pub(super) fn repo_level_404(repo: &RepoId, e: &ApiError) -> FlagError {
    if is_no_tracker(e) {
        tracker_disabled(repo)
    } else {
        repo_not_found(repo)
    }
}

/// Classify a 404 on an issue endpoint that names a specific issue (e.g.
/// `.../issues/{id}`). A "no issue tracker" body means the tracker is disabled;
/// a body pointing at the repository (no longer exists / no access) means the
/// repo is missing; otherwise the issue itself is missing.
pub(super) fn issue_level_404(repo: &RepoId, id: &str, e: &ApiError) -> FlagError {
    if is_no_tracker(e) {
        tracker_disabled(repo)
    } else if mentions_repo(e) {
        repo_not_found(repo)
    } else {
        FlagError::new(format!("issue #{id} not found"))
    }
}

/// Does this error's body point at the repository (rather than a single issue)?
/// Bitbucket's repo-level 404 reads like "...no longer exists, or you may not
/// have access..."; an issue-level 404 reads like "No such issue."
fn mentions_repo(e: &ApiError) -> bool {
    e.http_message().is_some_and(|m| {
        let m = m.to_ascii_lowercase();
        m.contains("no longer exists") || m.contains("not have access") || m.contains("repository")
    })
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
