//! `bb pr approve`.

use bb_api::BitbucketClient;
use bb_core::{Context, Method};
use clap::Args;

#[derive(Args, Debug)]
pub struct ApproveArgs {
    /// Pull request id (defaults to the PR for the current branch)
    #[arg(value_name = "ID")]
    pub id: Option<String>,
    /// Remove your approval instead of adding it
    #[arg(long)]
    pub undo: bool,
}

/// Run `bb pr approve` (or remove your approval with `--undo`).
///
/// # Errors
/// Returns [`bb_core::AuthError`] when not authenticated, or propagates the
/// API/git error.
pub fn run(ctx: &Context, args: ApproveArgs) -> anyhow::Result<()> {
    let repo = ctx.base_repo()?;
    let host = repo.host().to_owned();
    let header = crate::auth::header_for(ctx.config.as_ref(), &host);
    if header.is_none() {
        return Err(bb_core::AuthError::new(host).into());
    }
    let client = BitbucketClient::new(ctx.transport.clone(), header);

    let id = match args.id.as_deref() {
        Some(s) => super::finder::parse_id(s)?,
        None => super::finder::resolve(ctx, &client, &repo, None)?.id,
    };

    let path = format!(
        "/repositories/{}/{}/pullrequests/{id}/approve",
        repo.workspace(),
        repo.slug()
    );

    if args.undo {
        client.send_empty(Method::Delete, &path)?;
        ctx.io
            .println(&format!("✓ Removed your approval from #{id}"));
    } else {
        client.send_empty(Method::Post, &path)?;
        ctx.io.println(&format!("✓ Approved pull request #{id}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bb_api::testing::FakeTransport;
    use bb_config::FileConfig;
    use bb_core::{ConfigProvider, GitClient, RepoId, Transport};
    use bb_git::{ShellGit, StubRunner};

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
    ) -> (Context, bb_core::TestBuffers) {
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
    fn approve_posts_and_prints_confirmation() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "POST approve",
            FakeTransport::rest(Method::Post, "/pullrequests/42/approve"),
            FakeTransport::json(200, r#"{"approved":true}"#),
        );
        let (ctx, bufs) = ctx_with(h.clone(), authed_config());
        let args = ApproveArgs {
            id: Some("42".to_owned()),
            undo: false,
        };
        run(&ctx, args).unwrap();
        assert!(
            bufs.stdout_string().contains("Approved pull request #42"),
            "out: {}",
            bufs.stdout_string()
        );
    }

    #[test]
    fn approve_undo_sends_delete_and_prints_confirmation() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "DELETE approve",
            FakeTransport::rest(Method::Delete, "/pullrequests/42/approve"),
            FakeTransport::json(204, ""),
        );
        let (ctx, bufs) = ctx_with(h.clone(), authed_config());
        let args = ApproveArgs {
            id: Some("42".to_owned()),
            undo: true,
        };
        run(&ctx, args).unwrap();
        assert!(
            bufs.stdout_string()
                .contains("Removed your approval from #42"),
            "out: {}",
            bufs.stdout_string()
        );

        // Confirm the request was actually a DELETE.
        let reqs = h.requests.lock().unwrap();
        assert!(
            reqs.iter()
                .any(|r| r.method == Method::Delete && r.url.contains("/approve")),
            "expected a DELETE to /approve"
        );
    }

    #[test]
    fn approve_not_authed_returns_auth_error() {
        let h = Arc::new(FakeTransport::new());
        let (ctx, _bufs) = ctx_with(h.clone(), Arc::new(FileConfig::blank()));
        let args = ApproveArgs {
            id: Some("42".to_owned()),
            undo: false,
        };
        let err = run(&ctx, args).unwrap_err();
        assert!(
            err.downcast_ref::<bb_core::AuthError>().is_some(),
            "expected AuthError, got: {err:#}"
        );
    }
}
