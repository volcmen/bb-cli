//! `bb pr` — pull request commands.

mod approve;
mod checkout;
mod checks;
mod close;
mod create;
mod diff;
mod edit;
mod finder;
mod list;
mod merge;
mod render;
mod view;

use crate::core::Context;
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
    /// Edit a pull request's title, description, or base branch
    Edit(edit::EditArgs),
    /// List pull requests
    List(list::ListArgs),
    /// View a pull request
    View(view::ViewArgs),
    /// View a pull request's diff
    Diff(diff::DiffArgs),
    /// Merge a pull request
    Merge(merge::MergeArgs),
    /// Close (decline) a pull request
    Close(close::CloseArgs),
    /// Approve a pull request (or remove your approval)
    Approve(approve::ApproveArgs),
    /// Check out a pull request's branch locally
    Checkout(checkout::CheckoutArgs),
    /// Show CI/build checks for a pull request
    Checks(checks::ChecksArgs),
}

/// Dispatch `bb pr <sub>`.
///
/// # Errors
/// Propagates the sub-command's error.
pub fn run(ctx: &Context, args: PrArgs) -> anyhow::Result<()> {
    match args.command {
        PrCommands::Create(a) => create::run(ctx, a),
        PrCommands::Edit(a) => edit::run(ctx, a),
        PrCommands::List(a) => list::run(ctx, a),
        PrCommands::View(a) => view::run(ctx, a),
        PrCommands::Diff(a) => diff::run(ctx, a),
        PrCommands::Merge(a) => merge::run(ctx, a),
        PrCommands::Close(a) => close::run(ctx, a),
        PrCommands::Approve(a) => approve::run(ctx, a),
        PrCommands::Checkout(a) => checkout::run(ctx, a),
        PrCommands::Checks(a) => checks::run(ctx, a),
    }
}
