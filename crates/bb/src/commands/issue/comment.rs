//! `bb issue comment`.

use bb_core::Context;
use clap::Args;

#[derive(Args, Debug)]
pub struct CommentArgs {
    /// Issue id
    #[arg(value_name = "ID")]
    pub id: String,
    /// Comment body
    #[arg(long, short)]
    pub body: Option<String>,
    /// Read the comment body from a file (use "-" for stdin)
    #[arg(long = "body-file", short = 'F', value_name = "FILE")]
    pub body_file: Option<String>,
}

/// Run `bb issue comment`.
///
/// # Errors
/// TODO(#38): implement.
pub fn run(_ctx: &Context, _args: CommentArgs) -> anyhow::Result<()> {
    anyhow::bail!("`bb issue comment` is not implemented yet (#38)")
}
