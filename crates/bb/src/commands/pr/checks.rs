//! `bb pr checks` — show build statuses for a PR's head commit.

use bb_core::Context;
use clap::Args;

#[derive(Args, Debug)]
pub struct ChecksArgs {
    /// Pull request id (defaults to the PR for the current branch)
    #[arg(value_name = "ID")]
    pub id: Option<String>,
    #[command(flatten)]
    pub json: crate::output::JsonFlags,
}

/// Run `bb pr checks`.
///
/// # Errors
/// TODO(#42): implement.
pub fn run(_ctx: &Context, _args: ChecksArgs) -> anyhow::Result<()> {
    anyhow::bail!("`bb pr checks` is not implemented yet (#42)")
}
