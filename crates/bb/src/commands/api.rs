//! `bb api` — make an authenticated Bitbucket API request and print the result.

use bb_core::Context;
use clap::Args;

#[derive(Args, Debug)]
pub struct ApiArgs {
    /// API path, e.g. `/user` or `/repositories/WS/SLUG` (a full URL also works)
    #[arg(value_name = "PATH")]
    pub path: String,
    /// HTTP method
    #[arg(short = 'X', long = "method", default_value = "GET")]
    pub method: String,
    /// Add a string field `key=value` to a JSON request body (repeatable)
    #[arg(short = 'f', long = "field", value_name = "KEY=VALUE")]
    pub fields: Vec<String>,
    /// Follow pagination, concatenating each page's `values` into one array
    #[arg(long)]
    pub paginate: bool,
}

/// Run `bb api`.
///
/// # Errors
/// TODO(#45): implement.
pub fn run(_ctx: &Context, _args: ApiArgs) -> anyhow::Result<()> {
    anyhow::bail!("`bb api` is not implemented yet (#45)")
}
