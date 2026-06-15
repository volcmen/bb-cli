//! `bb auth login` — Basic (token paste) and OAuth 2.0 (`--web`).

use bb_core::Context;
use clap::Args;

#[derive(Args, Debug)]
pub struct LoginArgs {
    /// The Bitbucket host (default: bitbucket.org)
    #[arg(long)]
    pub hostname: Option<String>,
    /// Authenticate via OAuth 2.0 in the browser
    #[arg(long)]
    pub web: bool,
    /// Read the token / app password from standard input
    #[arg(long)]
    pub with_token: bool,
    /// Username (app password) or account email (API token)
    #[arg(long)]
    pub username: Option<String>,
    /// Credential type for Basic auth
    #[arg(long, value_parser = ["api_token", "app_password"])]
    pub auth_type: Option<String>,
}

/// Run `bb auth login`.
///
/// # Errors
/// TODO(#14): implement Basic + OAuth login.
pub fn run(_ctx: &Context, _args: LoginArgs) -> anyhow::Result<()> {
    anyhow::bail!("`bb auth login` is not implemented yet (#14)")
}
