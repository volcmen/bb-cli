//! Assembles the production [`Context`] by wiring the real implementations of
//! each seam trait (the analog of `gh`'s `factory.New`).

use std::sync::Arc;

use anyhow::Result;
use bb_api::ReqwestTransport;
use bb_core::{Context, IoStreams, RepoId, Transport};
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
    // Wrap the real transport so an expired OAuth token is refreshed and the
    // request retried transparently (seamless re-auth without re-running login).
    let inner: Arc<dyn Transport> = Arc::new(ReqwestTransport::new(&user_agent()));
    let transport = Arc::new(crate::refresh::RefreshingTransport::new(
        inner,
        config.clone(),
        bb_core::DEFAULT_HOST.to_owned(),
    ));
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
