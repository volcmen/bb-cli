//! `bb pr create`.

use bb_core::Context;
use clap::Args;

#[derive(Args, Debug)]
pub struct CreateArgs {
    /// Title for the pull request
    #[arg(long, short)]
    pub title: Option<String>,
    /// Body/description for the pull request
    #[arg(long, short)]
    pub body: Option<String>,
    /// Read the body from a file (use "-" for stdin)
    #[arg(long = "body-file", short = 'F', value_name = "FILE")]
    pub body_file: Option<String>,
    /// The destination (base) branch (default: repo main branch)
    #[arg(long, short = 'B')]
    pub base: Option<String>,
    /// The source (head) branch (default: current branch)
    #[arg(long, short = 'H')]
    pub head: Option<String>,
    /// Close the source branch after merge
    #[arg(long)]
    pub close_source_branch: bool,
    /// Open the new PR in the browser instead of creating via API
    #[arg(long)]
    pub web: bool,
    /// Request reviewers (comma-separated usernames)
    #[arg(long, value_delimiter = ',')]
    pub reviewer: Vec<String>,
}

/// Run `bb pr create`.
///
/// # Errors
/// TODO(#16): implement.
pub fn run(_ctx: &Context, _args: CreateArgs) -> anyhow::Result<()> {
    anyhow::bail!("`bb pr create` is not implemented yet (#16)")
}
