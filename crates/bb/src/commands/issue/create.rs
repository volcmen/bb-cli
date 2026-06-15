//! `bb issue create`.

use bb_core::Context;
use clap::Args;

#[derive(Args, Debug)]
pub struct CreateArgs {
    /// Issue title
    #[arg(long, short)]
    pub title: Option<String>,
    /// Issue body/content
    #[arg(long, short)]
    pub body: Option<String>,
    /// Read the body from a file (use "-" for stdin)
    #[arg(long = "body-file", short = 'F', value_name = "FILE")]
    pub body_file: Option<String>,
    /// Issue kind
    #[arg(long, value_parser = ["bug", "enhancement", "proposal", "task"])]
    pub kind: Option<String>,
    /// Issue priority
    #[arg(long, value_parser = ["trivial", "minor", "major", "critical", "blocker"])]
    pub priority: Option<String>,
}

/// Run `bb issue create`.
///
/// # Errors
/// TODO(#37): implement.
pub fn run(_ctx: &Context, _args: CreateArgs) -> anyhow::Result<()> {
    anyhow::bail!("`bb issue create` is not implemented yet (#37)")
}
