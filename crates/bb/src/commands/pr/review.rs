//! `bb pr review` — approve, request changes, or leave a review comment.

use crate::api::BitbucketClient;
use crate::core::{AuthError, Context, FlagError, Method};
use clap::Args;

#[derive(Args, Debug)]
pub struct ReviewArgs {
    /// Pull request id (defaults to the PR for the current branch)
    #[arg(value_name = "ID")]
    pub id: Option<String>,
    /// Approve the pull request
    #[arg(long)]
    pub approve: bool,
    /// Request changes on the pull request
    #[arg(long = "request-changes")]
    pub request_changes: bool,
    /// Leave a review comment (requires a body)
    #[arg(long)]
    pub comment: bool,
    /// Comment body (with --comment)
    #[arg(long, short = 'b')]
    pub body: Option<String>,
    /// Read the comment body from a file (use "-" for stdin)
    #[arg(long = "body-file", short = 'F', value_name = "FILE")]
    pub body_file: Option<String>,
}

#[derive(serde::Serialize)]
struct Content<'a> {
    raw: &'a str,
}

#[derive(serde::Serialize)]
struct CommentBody<'a> {
    content: Content<'a>,
}

/// Run `bb pr review`.
///
/// # Errors
/// Returns [`AuthError`] (exit 4) when not authenticated, [`FlagError`] (exit 1)
/// when not exactly one action is given / a comment has no body / the id is
/// invalid, and propagates [`ApiError`](crate::core::ApiError) / IO errors.
pub fn run(ctx: &Context, args: ReviewArgs) -> anyhow::Result<()> {
    let mode_count =
        usize::from(args.approve) + usize::from(args.request_changes) + usize::from(args.comment);
    if mode_count != 1 {
        return Err(FlagError::new(
            "specify exactly one of --approve, --request-changes, or --comment",
        )
        .into());
    }

    let repo = ctx.base_repo()?;
    let host = repo.host().to_owned();
    let Some(header) = crate::auth::header_for(ctx.config.as_ref(), &host) else {
        return Err(AuthError::new(host).into());
    };
    let client = BitbucketClient::new(ctx.transport.clone(), Some(header));

    let id = match args.id.as_deref() {
        Some(s) => super::finder::parse_id(s)?,
        None => super::finder::resolve(ctx, &client, &repo, None)?.id,
    };
    let base = format!(
        "/repositories/{}/{}/pullrequests/{id}",
        repo.workspace(),
        repo.slug()
    );

    if args.comment {
        let body = resolve_body(ctx, &args)?;
        let payload = CommentBody {
            content: Content { raw: &body },
        };
        let _resp: serde_json::Value = client.post(&format!("{base}/comments"), &payload)?;
        ctx.io
            .println(&format!("✓ Commented on pull request #{id}"));
    } else if args.approve {
        client.send_empty(Method::Post, &format!("{base}/approve"))?;
        ctx.io.println(&format!("✓ Approved pull request #{id}"));
    } else {
        client.send_empty(Method::Post, &format!("{base}/request-changes"))?;
        ctx.io
            .println(&format!("✓ Requested changes on pull request #{id}"));
    }
    Ok(())
}

/// Resolve the comment body from `--body`, then `--body-file` (`-` => stdin),
/// else an editor prompt when interactive, else a [`FlagError`].
fn resolve_body(ctx: &Context, args: &ReviewArgs) -> anyhow::Result<String> {
    if let Some(b) = &args.body {
        return Ok(b.clone());
    }
    if let Some(file) = &args.body_file {
        if file == "-" {
            return Ok(ctx.io.read_stdin_to_string()?);
        }
        return Ok(std::fs::read_to_string(file)?);
    }
    if ctx.io.can_prompt() {
        return ctx.prompter.editor("Review comment", "").map_err(to_anyhow);
    }
    Err(FlagError::new("--comment requires --body/--body-file when not interactive").into())
}

fn to_anyhow(err: crate::core::PromptError) -> anyhow::Error {
    match err {
        crate::core::PromptError::Cancelled => crate::core::CancelError.into(),
        other => anyhow::anyhow!(other),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::api::testing::FakeTransport;
    use crate::config::FileConfig;
    use crate::core::{ConfigProvider, GitClient, RepoId, Transport};
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

    fn git() -> Arc<dyn GitClient> {
        Arc::new(ShellGit::new(Arc::new(StubRunner::new())))
    }

    fn ctx_with(
        http: Arc<FakeTransport>,
        config: Arc<dyn ConfigProvider>,
    ) -> (Context, crate::core::TestBuffers) {
        let transport: Arc<dyn Transport> = http;
        let (mut ctx, bufs) = test_context(
            transport,
            git(),
            config,
            Arc::new(ScriptedPrompter::new()),
            false,
        );
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));
        (ctx, bufs)
    }

    fn args() -> ReviewArgs {
        ReviewArgs {
            id: Some("42".to_owned()),
            approve: false,
            request_changes: false,
            comment: false,
            body: None,
            body_file: None,
        }
    }

    #[test]
    fn review_approve_posts() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "approve",
            FakeTransport::rest(Method::Post, "/pullrequests/42/approve"),
            FakeTransport::json(200, r#"{"approved":true}"#),
        );
        let (ctx, bufs) = ctx_with(h.clone(), authed_config());
        run(
            &ctx,
            ReviewArgs {
                approve: true,
                ..args()
            },
        )
        .unwrap();
        assert!(bufs.stdout_string().contains("✓ Approved pull request #42"));
    }

    #[test]
    fn review_request_changes_posts() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "request-changes",
            FakeTransport::rest(Method::Post, "/pullrequests/42/request-changes"),
            FakeTransport::json(200, r#"{"type":"changes_requested"}"#),
        );
        let (ctx, bufs) = ctx_with(h.clone(), authed_config());
        run(
            &ctx,
            ReviewArgs {
                request_changes: true,
                ..args()
            },
        )
        .unwrap();
        assert!(bufs
            .stdout_string()
            .contains("✓ Requested changes on pull request #42"));
    }

    #[test]
    fn review_comment_posts_body() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "comment",
            FakeTransport::rest(Method::Post, "/pullrequests/42/comments"),
            FakeTransport::json(201, r#"{"id":1}"#),
        );
        let (ctx, _bufs) = ctx_with(h.clone(), authed_config());
        run(
            &ctx,
            ReviewArgs {
                comment: true,
                body: Some("nice".to_owned()),
                ..args()
            },
        )
        .unwrap();
        let reqs = h.requests.lock().unwrap();
        let post = reqs.iter().find(|r| r.method == Method::Post).unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(post.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["content"]["raw"], "nice");
    }

    #[test]
    fn review_no_mode_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        let (ctx, _bufs) = ctx_with(h.clone(), authed_config());
        let err = run(&ctx, args()).unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
        assert_eq!(h.request_count(), 0);
    }

    #[test]
    fn review_multiple_modes_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        let (ctx, _bufs) = ctx_with(h.clone(), authed_config());
        let err = run(
            &ctx,
            ReviewArgs {
                approve: true,
                request_changes: true,
                ..args()
            },
        )
        .unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
        assert_eq!(h.request_count(), 0);
    }

    #[test]
    fn review_comment_without_body_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        let (ctx, _bufs) = ctx_with(h.clone(), authed_config());
        let err = run(
            &ctx,
            ReviewArgs {
                comment: true,
                ..args()
            },
        )
        .unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
    }

    #[test]
    fn review_not_authed_is_auth_error() {
        let h = Arc::new(FakeTransport::new());
        let (ctx, _bufs) = ctx_with(h.clone(), Arc::new(FileConfig::blank()));
        let err = run(
            &ctx,
            ReviewArgs {
                approve: true,
                ..args()
            },
        )
        .unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some(), "got: {err}");
    }
}
