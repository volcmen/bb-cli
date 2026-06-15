//! `bb pr list`.

use bb_core::Context;
use clap::Args;

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Filter by state
    #[arg(long, default_value = "OPEN", value_parser = ["OPEN", "MERGED", "DECLINED", "SUPERSEDED"])]
    pub state: String,
    /// Maximum number of pull requests to list
    #[arg(long, short = 'L', default_value_t = 30)]
    pub limit: usize,
    /// Filter by destination (base) branch
    #[arg(long)]
    pub base: Option<String>,
}

/// Run `bb pr list`.
///
/// # Errors
/// TODO(#17): implement.
pub fn run(_ctx: &Context, _args: ListArgs) -> anyhow::Result<()> {
    anyhow::bail!("`bb pr list` is not implemented yet (#17)")
}
