//! `bb repo clone`.

use bb_core::Context;
use clap::Args;

#[derive(Args, Debug)]
pub struct CloneArgs {
    /// Repository as WORKSPACE/SLUG
    #[arg(value_name = "WORKSPACE/SLUG")]
    pub repo: String,
    /// Target directory (defaults to the repo slug)
    #[arg(value_name = "DIRECTORY")]
    pub dir: Option<String>,
}

/// Run `bb repo clone`.
///
/// # Errors
/// TODO(#28): implement.
pub fn run(_ctx: &Context, _args: CloneArgs) -> anyhow::Result<()> {
    anyhow::bail!("`bb repo clone` is not implemented yet (#28)")
}
