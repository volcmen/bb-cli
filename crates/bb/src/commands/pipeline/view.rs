//! `bb pipeline view`.

use bb_core::Context;
use clap::Args;

#[derive(Args, Debug)]
pub struct ViewArgs {
    /// Pipeline build number (or UUID)
    #[arg(value_name = "BUILD")]
    pub id: String,
    /// Also print each step's log
    #[arg(long)]
    pub log: bool,
    #[command(flatten)]
    pub json: crate::output::JsonFlags,
}

/// Run `bb pipeline view`.
///
/// # Errors
/// TODO(#41): implement.
pub fn run(_ctx: &Context, _args: ViewArgs) -> anyhow::Result<()> {
    anyhow::bail!("`bb pipeline view` is not implemented yet (#41)")
}
