//! `bb auth status` — show who you're logged in as.

use bb_core::Context;
use clap::Args;

#[derive(Args, Debug)]
pub struct StatusArgs {
    /// The Bitbucket host to check (default: configured hosts)
    #[arg(long)]
    pub hostname: Option<String>,
}

/// Run `bb auth status`.
///
/// # Errors
/// TODO(#15): implement.
pub fn run(_ctx: &Context, _args: StatusArgs) -> anyhow::Result<()> {
    anyhow::bail!("`bb auth status` is not implemented yet (#15)")
}
