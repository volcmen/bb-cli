//! `bb repo view`.

use bb_core::Context;
use clap::Args;

#[derive(Args, Debug)]
pub struct ViewArgs {
    /// Repository as WORKSPACE/SLUG (defaults to the current repo)
    #[arg(value_name = "WORKSPACE/SLUG")]
    pub repo: Option<String>,
    /// Open the repository in the browser
    #[arg(long)]
    pub web: bool,
}

/// Run `bb repo view`.
///
/// # Errors
/// TODO(#27): implement.
pub fn run(_ctx: &Context, _args: ViewArgs) -> anyhow::Result<()> {
    anyhow::bail!("`bb repo view` is not implemented yet (#27)")
}
