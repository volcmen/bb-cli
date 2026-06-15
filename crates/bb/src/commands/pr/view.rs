//! `bb pr view`.

use bb_core::Context;
use clap::Args;

#[derive(Args, Debug)]
pub struct ViewArgs {
    /// Pull request id (defaults to the PR for the current branch)
    #[arg(value_name = "ID")]
    pub id: Option<String>,
    /// Open the pull request in the browser
    #[arg(long)]
    pub web: bool,
}

/// Run `bb pr view`.
///
/// # Errors
/// TODO(#19): implement.
pub fn run(_ctx: &Context, _args: ViewArgs) -> anyhow::Result<()> {
    anyhow::bail!("`bb pr view` is not implemented yet (#19)")
}
