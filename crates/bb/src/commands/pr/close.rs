//! `bb pr close` (decline).

use bb_core::Context;
use clap::Args;

#[derive(Args, Debug)]
pub struct CloseArgs {
    /// Pull request id (defaults to the PR for the current branch)
    #[arg(value_name = "ID")]
    pub id: Option<String>,
    /// Reason for declining
    #[arg(long, short = 'm')]
    pub message: Option<String>,
}

/// Run `bb pr close`.
///
/// # Errors
/// TODO(#22): implement.
pub fn run(_ctx: &Context, _args: CloseArgs) -> anyhow::Result<()> {
    anyhow::bail!("`bb pr close` is not implemented yet (#22)")
}
