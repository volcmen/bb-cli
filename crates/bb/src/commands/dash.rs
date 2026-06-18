//! `bb dash` — launch the interactive TUI dashboard.

use crate::core::{Context, FlagError};
use clap::Args;

#[derive(Args, Debug)]
pub struct DashArgs {}

/// Run `bb dash`.
///
/// # Errors
/// [`FlagError`] (exit 1) when stdout is not an interactive terminal. Being
/// unauthenticated is **not** an error here — the dashboard renders a "Not logged
/// in" screen instead. Propagates terminal/IO errors from the TUI loop.
pub fn run(ctx: &Context, _args: DashArgs) -> anyhow::Result<()> {
    if !ctx.io.is_stdout_tty() {
        return Err(FlagError::new("bb dash requires an interactive terminal").into());
    }
    let host = ctx.host();
    let header = crate::auth::header_for(ctx.config.as_ref(), &host);
    let authed = header.is_some();
    // The repo is best-effort: dash still opens (with a hint) when none resolves.
    let repo = ctx.base_repo().ok();
    let (dash_config, warnings) = crate::tui::config::DashConfig::load(ctx.config.as_ref());
    crate::tui::run(
        authed,
        repo,
        ctx.transport.clone(),
        header,
        ctx.browser.clone(),
        dash_config,
        warnings,
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::api::testing::FakeTransport;
    use crate::config::FileConfig;
    use crate::core::{ConfigProvider, GitClient, Transport};
    use crate::git::{ShellGit, StubRunner};

    use super::*;
    use crate::testsupport::{test_context, ScriptedPrompter};

    #[test]
    fn dash_without_tty_is_flag_error() {
        let transport: Arc<dyn Transport> = Arc::new(FakeTransport::new());
        let git: Arc<dyn GitClient> = Arc::new(ShellGit::new(Arc::new(StubRunner::new())));
        let cfg: Arc<dyn ConfigProvider> = Arc::new(FileConfig::blank());
        // tty = false → not interactive.
        let (ctx, _bufs) = test_context(
            transport,
            git,
            cfg,
            Arc::new(ScriptedPrompter::new()),
            false,
        );

        let err = run(&ctx, DashArgs {}).unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
    }
}
