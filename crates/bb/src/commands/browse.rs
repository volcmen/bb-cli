//! `bb browse` — open a repository, pull request, branch, or commit in the
//! browser.

use bb_core::Context;
use clap::Args;

#[derive(Args, Debug)]
pub struct BrowseArgs {
    /// A pull request number to open (omit to open the repository)
    #[arg(value_name = "PR-NUMBER")]
    pub pr: Option<String>,
    /// Open the source view for a branch (no value = current branch)
    #[arg(long, value_name = "BRANCH", num_args = 0..=1, default_missing_value = "")]
    pub branch: Option<String>,
    /// Open a specific commit
    #[arg(long, value_name = "SHA")]
    pub commit: Option<String>,
    /// Open the repository's settings/admin page
    #[arg(long)]
    pub settings: bool,
    /// Print the destination URL instead of opening a browser
    #[arg(long)]
    pub no_browser: bool,
}

/// Run `bb browse`.
///
/// # Errors
/// TODO(#44): implement.
pub fn run(_ctx: &Context, _args: BrowseArgs) -> anyhow::Result<()> {
    anyhow::bail!("`bb browse` is not implemented yet (#44)")
}
