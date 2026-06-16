//! `bb pr close` (decline).

use crate::api::{BitbucketClient, PullRequest};
use crate::core::Context;
use clap::Args;

#[derive(Args, Debug)]
pub struct CloseArgs {
    /// Pull request id (defaults to the PR for the current branch)
    #[arg(value_name = "ID")]
    pub id: Option<String>,
    /// Reason for declining
    #[arg(long, short = 'm')]
    pub message: Option<String>,
}

/// The JSON body sent to `POST .../pullrequests/{id}/decline`.
#[derive(serde::Serialize)]
struct DeclineBody<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<&'a str>,
}

/// Run `bb pr close` (decline the pull request).
///
/// # Errors
/// Returns [`crate::core::AuthError`] when not authenticated, or propagates the
/// API/git error.
pub fn run(ctx: &Context, args: CloseArgs) -> anyhow::Result<()> {
    let repo = ctx.base_repo()?;
    let host = repo.host().to_owned();
    let header = crate::auth::header_for(ctx.config.as_ref(), &host);
    if header.is_none() {
        return Err(crate::core::AuthError::new(host).into());
    }
    let client = BitbucketClient::new(ctx.transport.clone(), header);

    let id = match args.id.as_deref() {
        Some(s) => super::finder::parse_id(s)?,
        None => super::finder::resolve(ctx, &client, &repo, None)?.id,
    };

    let body = DeclineBody {
        message: args.message.as_deref(),
    };
    let path = format!(
        "/repositories/{}/{}/pullrequests/{id}/decline",
        repo.workspace(),
        repo.slug()
    );
    let declined: PullRequest = client.post(&path, &body)?;

    let state = declined.state.as_deref().unwrap_or("DECLINED");
    ctx.io
        .println(&format!("✓ Declined pull request #{id} (state: {state})"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::api::testing::FakeTransport;
    use crate::config::FileConfig;
    use crate::core::{ConfigProvider, GitClient, Method, RepoId, Transport};
    use crate::git::{ShellGit, StubRunner};

    use super::*;
    use crate::testsupport::{test_context, ScriptedPrompter};

    const HOST: &str = "bitbucket.org";

    fn authed_config() -> Arc<dyn ConfigProvider> {
        let cfg = FileConfig::blank();
        cfg.set(HOST, "auth_type", "app_password").unwrap();
        cfg.set(HOST, "username", "u").unwrap();
        cfg.set(HOST, "token", "t").unwrap();
        Arc::new(cfg)
    }

    fn ctx_with(
        http: Arc<FakeTransport>,
        config: Arc<dyn ConfigProvider>,
    ) -> (Context, crate::core::TestBuffers) {
        let git: Arc<dyn GitClient> = Arc::new(ShellGit::new(Arc::new(StubRunner::new())));
        let transport: Arc<dyn Transport> = http;
        let (mut ctx, bufs) = test_context(
            transport,
            git,
            config,
            Arc::new(ScriptedPrompter::new()),
            false,
        );
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));
        (ctx, bufs)
    }

    #[test]
    fn close_happy_declines_and_prints_confirmation() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "POST decline",
            FakeTransport::rest(Method::Post, "/pullrequests/42/decline"),
            FakeTransport::json(200, r#"{"id":42,"title":"T","state":"DECLINED"}"#),
        );
        let (ctx, bufs) = ctx_with(h.clone(), authed_config());
        let args = CloseArgs {
            id: Some("42".to_owned()),
            message: None,
        };
        run(&ctx, args).unwrap();

        let out = bufs.stdout_string();
        assert!(
            out.contains("Declined pull request #42 (state: DECLINED)"),
            "out: {out}"
        );
    }

    #[test]
    fn close_not_authed_returns_auth_error() {
        let h = Arc::new(FakeTransport::new());
        let (ctx, _bufs) = ctx_with(h.clone(), Arc::new(FileConfig::blank()));
        let args = CloseArgs {
            id: Some("42".to_owned()),
            message: None,
        };
        let err = run(&ctx, args).unwrap_err();
        assert!(
            err.downcast_ref::<crate::core::AuthError>().is_some(),
            "expected AuthError, got: {err:#}"
        );
    }
}
