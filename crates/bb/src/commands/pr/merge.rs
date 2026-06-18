//! `bb pr merge`.

use crate::api::BitbucketClient;
use crate::core::Context;
use clap::Args;

use super::actions;

#[derive(Args, Debug)]
pub struct MergeArgs {
    /// Pull request id (defaults to the PR for the current branch)
    #[arg(value_name = "ID")]
    pub id: Option<String>,
    /// Merge strategy
    #[arg(long, default_value = "merge_commit", value_parser = ["merge_commit", "squash", "fast_forward"])]
    pub strategy: String,
    /// Close the source branch after merging
    #[arg(long)]
    pub close_source_branch: bool,
    /// Custom merge commit message
    #[arg(long, short = 'm')]
    pub message: Option<String>,
}

/// Run `bb pr merge`.
///
/// # Errors
/// Returns [`crate::core::AuthError`] when not authenticated, or propagates the
/// API/git error (e.g. a merge conflict surfaced by Bitbucket).
pub fn run(ctx: &Context, args: MergeArgs) -> anyhow::Result<()> {
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

    let result = actions::merge(
        &client,
        &repo,
        id,
        &args.strategy,
        args.message.as_deref(),
        args.close_source_branch,
    )?;

    match result.state.as_deref() {
        Some("MERGED") => {
            let mut line = format!("✓ Merged pull request #{id}");
            if let Some(url) = result.links.html_href() {
                line.push_str(&format!("\n{url}"));
            }
            ctx.io.println(&line);
        }
        Some(other) => {
            ctx.io
                .println(&format!("Pull request #{id} is now {other}"));
        }
        None => {
            ctx.io.println(&format!(
                "✓ Merge of pull request #{id} accepted (processing)"
            ));
        }
    }
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

    /// A context that resolves PRs by id (no branch inference) with the given
    /// config and a repo override of `acme/widgets`.
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
    fn merge_happy_sends_strategy_and_close_and_prints_confirmation() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "POST merge",
            FakeTransport::rest(Method::Post, "/pullrequests/42/merge"),
            FakeTransport::json(
                200,
                r#"{"id":42,"title":"T","state":"MERGED",
                    "links":{"html":{"href":"https://bitbucket.org/acme/widgets/pull-requests/42"}}}"#,
            ),
        );
        let (ctx, bufs) = ctx_with(h.clone(), authed_config());
        let args = MergeArgs {
            id: Some("42".to_owned()),
            strategy: "squash".to_owned(),
            close_source_branch: true,
            message: None,
        };
        run(&ctx, args).unwrap();

        let out = bufs.stdout_string();
        assert!(out.contains("Merged pull request #42"), "out: {out}");
        assert!(
            out.contains("https://bitbucket.org/acme/widgets/pull-requests/42"),
            "out: {out}"
        );

        // Inspect the JSON body of the POST.
        let reqs = h.requests.lock().unwrap();
        let post = reqs
            .iter()
            .find(|r| r.method == Method::Post && r.url.contains("/merge"))
            .expect("merge POST recorded");
        let raw = post.body.as_ref().expect("merge POST has a body");
        let body: serde_json::Value = serde_json::from_slice(raw).unwrap();
        assert_eq!(body["merge_strategy"], "squash");
        assert_eq!(body["close_source_branch"], true);
        assert!(body.get("message").is_none(), "message omitted when None");
    }

    #[test]
    fn merge_async_202_no_state_prints_accepted() {
        let h = Arc::new(FakeTransport::new());
        // A 202 task envelope with no `state` field (async merge).
        h.stub(
            "POST merge async",
            FakeTransport::rest(Method::Post, "/pullrequests/42/merge"),
            FakeTransport::json(202, r#"{"task_id":"abc-123","type":"merge_task"}"#),
        );
        let (ctx, bufs) = ctx_with(h.clone(), authed_config());
        let args = MergeArgs {
            id: Some("42".to_owned()),
            strategy: "merge_commit".to_owned(),
            close_source_branch: false,
            message: None,
        };
        run(&ctx, args).unwrap();
        let out = bufs.stdout_string();
        assert!(
            out.contains("Merge of pull request #42 accepted (processing)"),
            "out: {out}"
        );
    }

    #[test]
    fn merge_conflict_4xx_is_surfaced() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "POST merge conflict",
            FakeTransport::rest(Method::Post, "/pullrequests/42/merge"),
            FakeTransport::json(
                409,
                r#"{"error":{"message":"merge conflict: cannot merge"}}"#,
            ),
        );
        let (ctx, _bufs) = ctx_with(h.clone(), authed_config());
        let args = MergeArgs {
            id: Some("42".to_owned()),
            strategy: "merge_commit".to_owned(),
            close_source_branch: false,
            message: None,
        };
        let err = run(&ctx, args).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("merge conflict"), "err: {msg}");
    }

    #[test]
    fn merge_not_authed_returns_auth_error() {
        let h = Arc::new(FakeTransport::new());
        // No stubs: we must fail before any HTTP call.
        let (ctx, _bufs) = ctx_with(h.clone(), Arc::new(FileConfig::blank()));
        let args = MergeArgs {
            id: Some("42".to_owned()),
            strategy: "merge_commit".to_owned(),
            close_source_branch: false,
            message: None,
        };
        let err = run(&ctx, args).unwrap_err();
        assert!(
            err.downcast_ref::<crate::core::AuthError>().is_some(),
            "expected AuthError, got: {err:#}"
        );
    }
}
