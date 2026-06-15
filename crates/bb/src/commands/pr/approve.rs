//! `bb pr approve`.

use bb_core::Context;
use clap::Args;

#[derive(Args, Debug)]
pub struct ApproveArgs {
    /// Pull request id (defaults to the PR for the current branch)
    #[arg(value_name = "ID")]
    pub id: Option<String>,
    /// Remove your approval instead of adding it
    #[arg(long)]
    pub undo: bool,
}

/// Run `bb pr approve`.
///
/// # Errors
/// TODO(#23): implement.
pub fn run(_ctx: &Context, _args: ApproveArgs) -> anyhow::Result<()> {
    anyhow::bail!("`bb pr approve` is not implemented yet (#23)")
}
