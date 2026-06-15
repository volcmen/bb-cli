//! `bb pipeline list`.

use bb_core::Context;
use clap::Args;

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Maximum number of pipelines to list
    #[arg(long, short = 'L', default_value_t = 20)]
    pub limit: usize,
    #[command(flatten)]
    pub json: crate::output::JsonFlags,
}

/// Run `bb pipeline list`.
///
/// # Errors
/// TODO(#40): implement.
pub fn run(_ctx: &Context, _args: ListArgs) -> anyhow::Result<()> {
    anyhow::bail!("`bb pipeline list` is not implemented yet (#40)")
}
