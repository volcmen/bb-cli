//! `bb pr merge`.

use bb_core::Context;
use clap::Args;

#[derive(Args, Debug)]
pub struct MergeArgs {
    /// Pull request id (defaults to the PR for the current branch)
    #[arg(value_name = "ID")]
    pub id: Option<String>,
    /// Merge strategy
    #[arg(long, default_value = "merge_commit", value_parser = ["merge_commit", "squash", "fast_forward"])]
    pub strategy: String,
    /// Close the source branch after merging
    #[arg(long)]
    pub close_source_branch: bool,
    /// Custom merge commit message
    #[arg(long, short = 'm')]
    pub message: Option<String>,
}

/// Run `bb pr merge`.
///
/// # Errors
/// TODO(#21): implement.
pub fn run(_ctx: &Context, _args: MergeArgs) -> anyhow::Result<()> {
    anyhow::bail!("`bb pr merge` is not implemented yet (#21)")
}
