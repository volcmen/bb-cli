//! `bb auth setup-git` — configure git to authenticate HTTPS with `bb`.
//!
//! Mirrors `gh auth setup-git`: registers `bb auth git-credential` as the global
//! credential helper for the host, so `git`/`bb repo clone` over HTTPS use the
//! stored token (the HTTPS half of #92).

use crate::core::{Context, FlagError};
use clap::Args;

#[derive(Args, Debug)]
pub struct SetupGitArgs {
    /// The Bitbucket host to configure (default: the configured host)
    #[arg(long)]
    pub hostname: Option<String>,
}

/// Run `bb auth setup-git`.
///
/// # Errors
/// [`FlagError`] (1) when not logged in to the host; propagates
/// [`GitError`](crate::core::GitError) from writing the git config.
pub fn run(ctx: &Context, args: SetupGitArgs) -> anyhow::Result<()> {
    let host = args.hostname.unwrap_or_else(|| ctx.config.default_host());

    if ctx.config.get(&host, "token").is_none() {
        return Err(FlagError::new(format!(
            "not logged in to {host}; run `bb auth login` first"
        ))
        .into());
    }

    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(str::to_owned))
        .unwrap_or_else(|| "bb".to_owned());

    let key = format!("credential.https://{host}.helper");
    // Reset any existing helpers for this URL (empty value), then add ours — the
    // same two-step `gh` uses so bb's helper is the only one consulted.
    ctx.git.config_set_global(&key, "")?;
    ctx.git.config_add_global(&key, &helper_command(&exe))?;

    ctx.io.println(&format!(
        "✓ Configured git to use bb as a credential helper for {host}"
    ));
    Ok(())
}

/// The `credential.helper` value invoking `bb auth git-credential` (the leading
/// `!` makes git run it as a shell command). Paths with whitespace are quoted.
fn helper_command(exe: &str) -> String {
    if exe.contains(char::is_whitespace) {
        format!("!\"{exe}\" auth git-credential")
    } else {
        format!("!{exe} auth git-credential")
    }
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

    const HOST: &str = "bitbucket.org";

    fn authed() -> Arc<dyn ConfigProvider> {
        let cfg = FileConfig::blank();
        cfg.set(HOST, "auth_type", "app_password").unwrap();
        cfg.set(HOST, "token", "t").unwrap();
        Arc::new(cfg)
    }

    fn ctx_with(
        git: Arc<dyn GitClient>,
        cfg: Arc<dyn ConfigProvider>,
    ) -> (Context, crate::core::TestBuffers) {
        let transport: Arc<dyn Transport> = Arc::new(FakeTransport::new());
        test_context(
            transport,
            git,
            cfg,
            Arc::new(ScriptedPrompter::new()),
            false,
        )
    }

    #[test]
    fn setup_git_writes_reset_and_add() {
        let stub = Arc::new(StubRunner::new());
        // Reset (empty value): "credential" follows "--global" directly.
        stub.register(
            r"^git config --global credential\.https://bitbucket\.org\.helper",
            0,
            "",
        );
        // Add the bb helper.
        stub.register(
            r"^git config --global --add credential\.https://bitbucket\.org\.helper !.*auth git-credential",
            0,
            "",
        );
        let git: Arc<dyn GitClient> = Arc::new(ShellGit::new(stub));
        let (ctx, bufs) = ctx_with(git, authed());

        run(&ctx, SetupGitArgs { hostname: None }).unwrap();
        assert!(bufs
            .stdout_string()
            .contains("✓ Configured git to use bb as a credential helper for bitbucket.org"));
    }

    #[test]
    fn setup_git_not_logged_in_is_flag_error() {
        let git: Arc<dyn GitClient> = Arc::new(ShellGit::new(Arc::new(StubRunner::new())));
        let (ctx, _bufs) = ctx_with(git, Arc::new(FileConfig::blank()));

        let err = run(&ctx, SetupGitArgs { hostname: None }).unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
    }

    #[test]
    fn helper_command_quotes_paths_with_spaces() {
        assert_eq!(
            helper_command("/usr/bin/bb"),
            "!/usr/bin/bb auth git-credential"
        );
        assert_eq!(
            helper_command("/Apps/My Tools/bb"),
            "!\"/Apps/My Tools/bb\" auth git-credential"
        );
    }
}
