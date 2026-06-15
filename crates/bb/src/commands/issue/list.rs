//! `bb issue list`.

use bb_core::Context;
use clap::Args;

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Filter by state (new, open, resolved, closed, on_hold, invalid, duplicate, wontfix)
    #[arg(long)]
    pub state: Option<String>,
    /// Maximum number of issues to list
    #[arg(long, short = 'L', default_value_t = 30)]
    pub limit: usize,
    #[command(flatten)]
    pub json: crate::output::JsonFlags,
}

/// Run `bb issue list`.
///
/// # Errors
/// TODO(#35): implement.
pub fn run(_ctx: &Context, _args: ListArgs) -> anyhow::Result<()> {
    anyhow::bail!("`bb issue list` is not implemented yet (#35)")
}
