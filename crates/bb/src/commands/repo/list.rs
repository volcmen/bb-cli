//! `bb repo list`.

use bb_core::Context;
use clap::Args;

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Workspace to list repositories for (defaults to the current repo's workspace)
    #[arg(value_name = "WORKSPACE")]
    pub workspace: Option<String>,
    /// Maximum number of repositories to list
    #[arg(long, short = 'L', default_value_t = 30)]
    pub limit: usize,
}

/// Run `bb repo list`.
///
/// # Errors
/// TODO(#29): implement.
pub fn run(_ctx: &Context, _args: ListArgs) -> anyhow::Result<()> {
    anyhow::bail!("`bb repo list` is not implemented yet (#29)")
}
