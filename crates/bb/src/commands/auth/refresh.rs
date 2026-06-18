//! `bb auth refresh` — force an OAuth access-token refresh now.
//!
//! Reactive refresh already happens on a 401 (see [`crate::refresh`]); this lets
//! the user rotate the token on demand.

use crate::auth;
use crate::core::{AuthError, Context, FlagError};
use crate::render::percent_encode;
use clap::Args;

#[derive(Args, Debug)]
pub struct RefreshArgs {
    /// The Bitbucket host (default: the configured host)
    #[arg(long)]
    pub hostname: Option<String>,
}

/// Run `bb auth refresh`.
///
/// # Errors
/// [`AuthError`] (4) when not logged in to the host; [`FlagError`] (1) when the
/// host is not authenticated via OAuth or lacks the stored refresh credentials;
/// propagates the token-endpoint error and [`ConfigError`](crate::core::ConfigError).
pub fn run(ctx: &Context, args: RefreshArgs) -> anyhow::Result<()> {
    let host = args.hostname.unwrap_or_else(|| ctx.config.default_host());

    if ctx.config.get(&host, "token").is_none() {
        return Err(AuthError::new(host).into());
    }
    if ctx.config.get(&host, "auth_type").as_deref() != Some(auth::OAUTH) {
        return Err(FlagError::new(format!(
            "{host} is not authenticated via OAuth; nothing to refresh"
        ))
        .into());
    }

    let (Some(refresh_token), Some(client_id), Some(client_secret)) = (
        ctx.config.get(&host, "refresh_token"),
        ctx.config.get(&host, "oauth_client_id"),
        ctx.config.get(&host, "oauth_client_secret"),
    ) else {
        return Err(FlagError::new(format!(
            "no stored OAuth refresh credentials for {host}; run `bb auth login` again"
        ))
        .into());
    };

    let body = format!(
        "grant_type=refresh_token&refresh_token={}",
        percent_encode(&refresh_token)
    );
    let basic = auth::basic_header(&client_id, &client_secret);
    let token: auth::TokenResponse =
        auth::post_form(ctx.transport.as_ref(), auth::TOKEN_URL, &body, &basic)?;

    ctx.config.set(&host, "token", &token.access_token)?;
    if let Some(rt) = &token.refresh_token {
        ctx.config.set(&host, "refresh_token", rt)?;
    }
    ctx.config.save()?;

    ctx.io
        .println(&format!("✓ Refreshed the OAuth token for {host}"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::api::testing::FakeTransport;
    use crate::config::FileConfig;
    use crate::core::{ConfigProvider, GitClient, Method, Transport};
    use crate::git::{ShellGit, StubRunner};

    use super::*;
    use crate::testsupport::{test_context, ScriptedPrompter};

    const HOST: &str = "bitbucket.org";

    fn git() -> Arc<dyn GitClient> {
        Arc::new(ShellGit::new(Arc::new(StubRunner::new())))
    }

    fn ctx_with(
        http: Arc<FakeTransport>,
        cfg: Arc<dyn ConfigProvider>,
    ) -> (Context, crate::core::TestBuffers) {
        let transport: Arc<dyn Transport> = http;
        test_context(
            transport,
            git(),
            cfg,
            Arc::new(ScriptedPrompter::new()),
            false,
        )
    }

    fn oauth_config() -> (Arc<FileConfig>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let cfg = FileConfig::load_from(dir.path().to_path_buf()).unwrap();
        cfg.set(HOST, "auth_type", auth::OAUTH).unwrap();
        cfg.set(HOST, "token", "old-access").unwrap();
        cfg.set(HOST, "refresh_token", "rt-1").unwrap();
        cfg.set(HOST, "oauth_client_id", "cid").unwrap();
        cfg.set(HOST, "oauth_client_secret", "csec").unwrap();
        (Arc::new(cfg), dir)
    }

    #[test]
    fn refresh_oauth_persists_new_token() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "refresh grant",
            FakeTransport::rest(Method::Post, "/site/oauth2/access_token"),
            FakeTransport::json(
                200,
                r#"{"access_token":"new-access","refresh_token":"rt-2"}"#,
            ),
        );
        let (cfg, _d) = oauth_config();
        let (ctx, bufs) = ctx_with(h, cfg.clone());

        run(&ctx, RefreshArgs { hostname: None }).unwrap();

        assert_eq!(cfg.get(HOST, "token").as_deref(), Some("new-access"));
        assert_eq!(cfg.get(HOST, "refresh_token").as_deref(), Some("rt-2"));
        assert!(bufs
            .stdout_string()
            .contains("✓ Refreshed the OAuth token for bitbucket.org"));
    }

    #[test]
    fn refresh_non_oauth_is_flag_error() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = FileConfig::load_from(dir.path().to_path_buf()).unwrap();
        cfg.set(HOST, "auth_type", "app_password").unwrap();
        cfg.set(HOST, "token", "pw").unwrap();
        let (ctx, _bufs) = ctx_with(Arc::new(FakeTransport::new()), Arc::new(cfg));

        let err = run(&ctx, RefreshArgs { hostname: None }).unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
    }

    #[test]
    fn refresh_not_logged_in_is_auth_error() {
        let (ctx, _bufs) = ctx_with(
            Arc::new(FakeTransport::new()),
            Arc::new(FileConfig::blank()),
        );
        let err = run(
            &ctx,
            RefreshArgs {
                hostname: Some(HOST.to_owned()),
            },
        )
        .unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some(), "got: {err}");
    }
}
