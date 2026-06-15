//! Assembles the production [`Context`] by wiring the real implementations of
//! each seam trait (the analog of `gh`'s `factory.New`).

use std::sync::Arc;

use anyhow::Result;
use bb_api::ReqwestTransport;
use bb_core::{Context, IoStreams, RepoId};
use bb_git::ShellGit;

use crate::browser::SystemBrowser;
use crate::prompt::InquirePrompter;

/// The HTTP user agent sent on every request.
#[must_use]
pub fn user_agent() -> String {
    format!("bb-cli/{}", env!("CARGO_PKG_VERSION"))
}

/// Build the production context.
///
/// # Errors
/// Returns an error if config fails to load.
pub fn build_context(repo_override: Option<RepoId>) -> Result<Context> {
    let io = Arc::new(IoStreams::system());
    let config = bb_config::load()?;
    let transport = Arc::new(ReqwestTransport::new(&user_agent()));
    let git = Arc::new(ShellGit::system());
    let prompter = Arc::new(InquirePrompter);
    let browser = Arc::new(SystemBrowser);

    Ok(Context {
        io,
        prompter,
        browser,
        git,
        config,
        transport,
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
        repo_override,
    })
}
