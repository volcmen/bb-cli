//! `bb auth logout` — clear stored credentials for a host.

use crate::core::{Context, FlagError};
use clap::Args;

#[derive(Args, Debug)]
pub struct LogoutArgs {
    /// The Bitbucket host to log out of (default: the configured host)
    #[arg(long)]
    pub hostname: Option<String>,
}

/// Run `bb auth logout`.
///
/// # Errors
/// [`FlagError`] (1) when not logged in to the host; propagates
/// [`ConfigError`](crate::core::ConfigError) on save.
pub fn run(ctx: &Context, args: LogoutArgs) -> anyhow::Result<()> {
    let host = args.hostname.unwrap_or_else(|| ctx.config.default_host());
    if ctx.config.get(&host, "token").is_none() {
        return Err(FlagError::new(format!("not logged in to {host}")).into());
    }
    ctx.config.unset_host(&host)?;
    ctx.config.save()?;
    ctx.io.println(&format!("✓ Logged out of {host}"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::api::testing::FakeTransport;
    use crate::config::FileConfig;
    use crate::core::{ConfigProvider, FlagError, GitClient, Transport};
    use crate::git::{ShellGit, StubRunner};

    use super::*;
    use crate::testsupport::{test_context, ScriptedPrompter};

    fn git() -> Arc<dyn GitClient> {
        Arc::new(ShellGit::new(Arc::new(StubRunner::new())))
    }

    /// Temp-backed config (so `save()` is safe) seeded with creds.
    fn seeded() -> (Arc<FileConfig>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let cfg = FileConfig::load_from(dir.path().to_path_buf()).unwrap();
        cfg.set("bitbucket.org", "auth_type", "app_password")
            .unwrap();
        cfg.set("bitbucket.org", "token", "secret").unwrap();
        (Arc::new(cfg), dir)
    }

    #[test]
    fn logout_clears_host() {
        let (cfg, _dir) = seeded();
        let config: Arc<dyn ConfigProvider> = cfg.clone();
        let transport: Arc<dyn Transport> = Arc::new(FakeTransport::new());
        let (ctx, bufs) = test_context(
            transport,
            git(),
            config,
            Arc::new(ScriptedPrompter::new()),
            false,
        );

        run(&ctx, LogoutArgs { hostname: None }).unwrap();

        assert!(bufs
            .stdout_string()
            .contains("✓ Logged out of bitbucket.org"));
        assert!(
            cfg.get("bitbucket.org", "token").is_none(),
            "token should be cleared"
        );
    }

    #[test]
    fn logout_not_logged_in_is_flag_error() {
        let dir = tempfile::tempdir().unwrap();
        let config: Arc<dyn ConfigProvider> =
            Arc::new(FileConfig::load_from(dir.path().to_path_buf()).unwrap());
        let transport: Arc<dyn Transport> = Arc::new(FakeTransport::new());
        let (ctx, _bufs) = test_context(
            transport,
            git(),
            config,
            Arc::new(ScriptedPrompter::new()),
            false,
        );

        let err = run(
            &ctx,
            LogoutArgs {
                hostname: Some("bitbucket.org".to_owned()),
            },
        )
        .unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
    }
}
