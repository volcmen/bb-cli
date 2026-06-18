//! `bb auth token` — print the stored token for a host.

use crate::core::{AuthError, Context};
use clap::Args;

#[derive(Args, Debug)]
pub struct TokenArgs {
    /// The Bitbucket host (default: the configured host)
    #[arg(long)]
    pub hostname: Option<String>,
}

/// Run `bb auth token`.
///
/// # Errors
/// [`AuthError`] (4) when not logged in to the host.
pub fn run(ctx: &Context, args: TokenArgs) -> anyhow::Result<()> {
    let host = args.hostname.unwrap_or_else(|| ctx.config.default_host());
    match ctx.config.get(&host, "token") {
        Some(token) => {
            ctx.io.println(&token);
            Ok(())
        }
        None => Err(AuthError::new(host).into()),
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

    fn git() -> Arc<dyn GitClient> {
        Arc::new(ShellGit::new(Arc::new(StubRunner::new())))
    }

    fn ctx_with(config: Arc<dyn ConfigProvider>) -> (Context, crate::core::TestBuffers) {
        let transport: Arc<dyn Transport> = Arc::new(FakeTransport::new());
        test_context(
            transport,
            git(),
            config,
            Arc::new(ScriptedPrompter::new()),
            false,
        )
    }

    #[test]
    fn token_prints_stored_token() {
        let cfg = FileConfig::blank();
        cfg.set("bitbucket.org", "auth_type", "app_password")
            .unwrap();
        cfg.set("bitbucket.org", "token", "s3cr3t").unwrap();
        let (ctx, bufs) = ctx_with(Arc::new(cfg));

        run(&ctx, TokenArgs { hostname: None }).unwrap();
        assert_eq!(bufs.stdout_string().trim_end(), "s3cr3t");
    }

    #[test]
    fn token_not_logged_in_is_auth_error() {
        let (ctx, _bufs) = ctx_with(Arc::new(FileConfig::blank()));
        let err = run(
            &ctx,
            TokenArgs {
                hostname: Some("bitbucket.org".to_owned()),
            },
        )
        .unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some(), "got: {err}");
    }
}
