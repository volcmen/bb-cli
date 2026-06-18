//! `bb repo` — repository commands.

mod branch_restriction;
mod clone;
mod create;
mod default_reviewer;
mod delete;
mod deploy_key;
mod edit;
mod fork;
mod list;
mod set_default;
mod sync;
mod view;
mod webhook;

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
    /// Delete a repository
    Delete(delete::DeleteArgs),
    /// List repositories in a workspace
    List(list::ListArgs),
    /// Set the default repository for the current directory
    SetDefault(set_default::SetDefaultArgs),
    /// Sync a fork's current branch with its upstream
    Sync(sync::SyncArgs),
    /// Manage repository webhooks
    Webhook(webhook::WebhookArgs),
    /// Manage repository deploy keys
    DeployKey(deploy_key::DeployKeyArgs),
    /// Manage branch restrictions (branch protection)
    BranchRestriction(branch_restriction::BranchRestrictionArgs),
    /// Manage default reviewers
    DefaultReviewer(default_reviewer::DefaultReviewerArgs),
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
        RepoCommands::Delete(a) => delete::run(ctx, a),
        RepoCommands::List(a) => list::run(ctx, a),
        RepoCommands::SetDefault(a) => set_default::run(ctx, a),
        RepoCommands::Sync(a) => sync::run(ctx, a),
        RepoCommands::Webhook(a) => webhook::run(ctx, a),
        RepoCommands::DeployKey(a) => deploy_key::run(ctx, a),
        RepoCommands::BranchRestriction(a) => branch_restriction::run(ctx, a),
        RepoCommands::DefaultReviewer(a) => default_reviewer::run(ctx, a),
    }
}
