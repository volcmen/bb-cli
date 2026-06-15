//! `bb pr checkout`.

use bb_core::Context;
use clap::Args;

#[derive(Args, Debug)]
pub struct CheckoutArgs {
    /// Pull request id
    #[arg(value_name = "ID")]
    pub id: String,
}

/// Run `bb pr checkout`.
///
/// # Errors
/// TODO(#25): implement.
pub fn run(_ctx: &Context, _args: CheckoutArgs) -> anyhow::Result<()> {
    anyhow::bail!("`bb pr checkout` is not implemented yet (#25)")
}
