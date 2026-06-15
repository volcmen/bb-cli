//! `bb pr diff`.

use bb_core::Context;
use clap::Args;

#[derive(Args, Debug)]
pub struct DiffArgs {
    /// Pull request id (defaults to the PR for the current branch)
    #[arg(value_name = "ID")]
    pub id: Option<String>,
}

/// Run `bb pr diff`.
///
/// # Errors
/// TODO(#20): implement.
pub fn run(_ctx: &Context, _args: DiffArgs) -> anyhow::Result<()> {
    anyhow::bail!("`bb pr diff` is not implemented yet (#20)")
}
