//! `bb issue view`.

use bb_core::Context;
use clap::Args;

#[derive(Args, Debug)]
pub struct ViewArgs {
    /// Issue id
    #[arg(value_name = "ID")]
    pub id: String,
    /// Open the issue in the browser
    #[arg(long)]
    pub web: bool,
    #[command(flatten)]
    pub json: crate::output::JsonFlags,
}

/// Run `bb issue view`.
///
/// # Errors
/// TODO(#36): implement.
pub fn run(_ctx: &Context, _args: ViewArgs) -> anyhow::Result<()> {
    anyhow::bail!("`bb issue view` is not implemented yet (#36)")
}
