//! `bb issue comment`.

use crate::api::BitbucketClient;
use crate::core::{AuthError, Context, FlagError};
use clap::Args;

#[derive(Args, Debug)]
pub struct CommentArgs {
    /// Issue id
    #[arg(value_name = "ID")]
    pub id: String,
    /// Comment body
    #[arg(long, short)]
    pub body: Option<String>,
    /// Read the comment body from a file (use "-" for stdin)
    #[arg(long = "body-file", short = 'F', value_name = "FILE")]
    pub body_file: Option<String>,
}

// ----- request body shapes ----------------------------------------------

/// A `{ "raw": ... }` content wrapper, matching Bitbucket's rendered-content shape.
#[derive(serde::Serialize)]
struct Content<'a> {
    raw: &'a str,
}

/// The JSON body sent to `POST .../issues/{id}/comments`.
#[derive(serde::Serialize)]
struct CommentBody<'a> {
    content: Content<'a>,
}

/// Run `bb issue comment`.
///
/// # Errors
/// Returns [`AuthError`] (exit 4) when not authenticated, [`FlagError`] for an
/// invalid issue id or a missing body when non-interactive, and propagates
/// [`ApiError`](crate::core::ApiError) / IO errors.
pub fn run(ctx: &Context, args: CommentArgs) -> anyhow::Result<()> {
    let repo = ctx.base_repo()?;
    let host = repo.host().to_owned();

    let Some(header) = crate::auth::header_for(ctx.config.as_ref(), &host) else {
        return Err(AuthError::new(host).into());
    };
    let client = BitbucketClient::new(ctx.transport.clone(), Some(header));

    let id: u64 = args
        .id
        .parse()
        .map_err(|_| FlagError::new("invalid issue id"))?;

    // body: --body, else --body-file (`-` => stdin), else editor when
    // interactive, else FlagError.
    let body = resolve_body(ctx, &args)?;

    let payload = CommentBody {
        content: Content { raw: &body },
    };

    let path = format!(
        "/repositories/{}/{}/issues/{id}/comments",
        repo.workspace(),
        repo.slug()
    );
    // Comment response shape is loose; we don't need any of its fields.
    let id_str = id.to_string();
    let _resp: serde_json::Value = match client.post(&path, &payload) {
        Ok(v) => v,
        // 410 = tracker disabled. A 404 is ambiguous: disabled tracker, missing
        // repo, or missing issue — decide by the body message.
        Err(e) if e.is_gone() => return Err(super::tracker_disabled(&repo).into()),
        Err(e) if e.is_not_found() => {
            return Err(super::issue_level_404(&repo, &id_str, &e).into());
        }
        Err(e) => return Err(e.into()),
    };

    ctx.io.println(&format!("✓ Commented on issue #{id}"));
    Ok(())
}

/// Resolve the comment body from `--body`, then `--body-file` (`-` => stdin),
/// else an editor prompt when interactive, else a [`FlagError`].
fn resolve_body(ctx: &Context, args: &CommentArgs) -> anyhow::Result<String> {
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
        return ctx.prompter.editor("Comment", "").map_err(to_anyhow);
    }
    Err(FlagError::new("--body required when not running interactively").into())
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
    use crate::core::{ConfigProvider, GitClient, IoStreams, Method, Prompter, RepoId, Transport};
    use crate::git::{ShellGit, StubRunner};

    use super::*;
    use crate::testsupport::{test_context, RecordingBrowser, ScriptedPrompter};

    const HOST: &str = "bitbucket.org";

    fn authed_config() -> Arc<dyn ConfigProvider> {
        let cfg = FileConfig::blank();
        cfg.set(HOST, "auth_type", "app_password").unwrap();
        cfg.set(HOST, "username", "u").unwrap();
        cfg.set(HOST, "token", "t").unwrap();
        Arc::new(cfg)
    }

    /// Non-interactive context (`tty=false`, `can_prompt()` is false).
    fn ctx_with(
        http: Arc<FakeTransport>,
        config: Arc<dyn ConfigProvider>,
        prompter: Arc<dyn Prompter>,
    ) -> (Context, crate::core::TestBuffers) {
        let git: Arc<dyn GitClient> = Arc::new(ShellGit::new(Arc::new(StubRunner::new())));
        let transport: Arc<dyn Transport> = http;
        let (mut ctx, bufs) = test_context(transport, git, config, prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));
        (ctx, bufs)
    }

    /// Interactive context where `can_prompt()` is true. `test_context` leaves
    /// `never_prompt` set, so build the context directly (mirrors `auth login`).
    fn interactive_ctx(
        http: Arc<FakeTransport>,
        config: Arc<dyn ConfigProvider>,
        prompter: Arc<dyn Prompter>,
    ) -> (Context, crate::core::TestBuffers) {
        let (mut io, bufs) = IoStreams::test();
        io.set_stdout_tty(true);
        io.set_stderr_tty(true);
        io.set_stdin_tty(true);
        io.set_never_prompt(false);
        let git: Arc<dyn GitClient> = Arc::new(ShellGit::new(Arc::new(StubRunner::new())));
        let ctx = Context {
            io: Arc::new(io),
            prompter,
            browser: Arc::new(RecordingBrowser::default()),
            git,
            config,
            transport: http,
            app_version: "test".to_owned(),
            repo_override: Some(RepoId::new("acme", "widgets")),
        };
        (ctx, bufs)
    }

    #[test]
    fn comment_happy_posts_content_and_prints_confirmation() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "post comment",
            FakeTransport::rest(Method::Post, "/repositories/acme/widgets/issues/7/comments"),
            FakeTransport::json(201, r#"{"id": 100, "content": {"raw": "thanks"}}"#),
        );
        let (ctx, bufs) = ctx_with(
            h.clone(),
            authed_config(),
            Arc::new(ScriptedPrompter::new()),
        );

        let a = CommentArgs {
            id: "7".to_owned(),
            body: Some("thanks".to_owned()),
            body_file: None,
        };
        run(&ctx, a).unwrap();

        let out = bufs.stdout_string();
        assert!(out.contains("✓ Commented on issue #7"), "out: {out}");

        let reqs = h.requests.lock().unwrap();
        let post = reqs
            .iter()
            .find(|r| r.method == Method::Post)
            .expect("a POST");
        let body: serde_json::Value =
            serde_json::from_slice(post.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["content"]["raw"], "thanks");
    }

    #[test]
    fn comment_prompts_editor_when_interactive() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "post comment",
            FakeTransport::rest(Method::Post, "/repositories/acme/widgets/issues/3/comments"),
            FakeTransport::json(201, r#"{"id": 1}"#),
        );
        let prompter = Arc::new(ScriptedPrompter::new().editor("from editor"));
        let (ctx, bufs) = interactive_ctx(h.clone(), authed_config(), prompter);

        let a = CommentArgs {
            id: "3".to_owned(),
            body: None,
            body_file: None,
        };
        run(&ctx, a).unwrap();

        assert!(bufs.stdout_string().contains("✓ Commented on issue #3"));
        let reqs = h.requests.lock().unwrap();
        let post = reqs.iter().find(|r| r.method == Method::Post).unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(post.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["content"]["raw"], "from editor");
    }

    #[test]
    fn comment_tracker_disabled_410_is_flag_error() {
        // #77: commenting on a disabled tracker returns 410 Gone.
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "comment 410",
            FakeTransport::rest(Method::Post, "/repositories/acme/widgets/issues/7/comments"),
            FakeTransport::json(410, r#"{"type":"error","error":{"message":"Gone"}}"#),
        );
        let (ctx, _bufs) = ctx_with(
            h.clone(),
            authed_config(),
            Arc::new(ScriptedPrompter::new()),
        );

        let a = CommentArgs {
            id: "7".to_owned(),
            body: Some("hi".to_owned()),
            body_file: None,
        };
        let err = run(&ctx, a).unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
        assert!(
            err.to_string().contains("issue tracker is not enabled"),
            "msg: {err}"
        );
    }

    #[test]
    fn comment_tracker_disabled_404_body_is_flag_error() {
        // A 404 whose body says the repo has no issue tracker => tracker-disabled.
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "comment 404 no tracker",
            FakeTransport::rest(Method::Post, "/repositories/acme/widgets/issues/7/comments"),
            FakeTransport::json(
                404,
                r#"{"type":"error","error":{"message":"Repository has no issue tracker."}}"#,
            ),
        );
        let (ctx, _bufs) = ctx_with(
            h.clone(),
            authed_config(),
            Arc::new(ScriptedPrompter::new()),
        );

        let a = CommentArgs {
            id: "7".to_owned(),
            body: Some("hi".to_owned()),
            body_file: None,
        };
        let err = run(&ctx, a).unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
        assert!(
            err.to_string().contains("issue tracker is not enabled"),
            "msg: {err}"
        );
    }

    #[test]
    fn comment_repo_not_found_404_reports_repo() {
        // #97: a 404 pointing at the repository must report a missing repo.
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "comment 404 missing repo",
            FakeTransport::rest(Method::Post, "/repositories/acme/widgets/issues/7/comments"),
            FakeTransport::json(
                404,
                r#"{"type":"error","error":{"message":"Repository acme/widgets no longer exists, or you may not have access."}}"#,
            ),
        );
        let (ctx, _bufs) = ctx_with(
            h.clone(),
            authed_config(),
            Arc::new(ScriptedPrompter::new()),
        );

        let a = CommentArgs {
            id: "7".to_owned(),
            body: Some("hi".to_owned()),
            body_file: None,
        };
        let err = run(&ctx, a).unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
        let msg = err.to_string();
        assert!(msg.contains("not found"), "should report not-found: {msg}");
        assert!(!msg.contains("tracker"), "must not say tracker: {msg}");
    }

    #[test]
    fn comment_issue_not_found_404_reports_issue() {
        // A 404 naming the issue (not repo/tracker) => the issue is missing.
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "comment 404 missing issue",
            FakeTransport::rest(Method::Post, "/repositories/acme/widgets/issues/7/comments"),
            FakeTransport::json(
                404,
                r#"{"type":"error","error":{"message":"No such issue."}}"#,
            ),
        );
        let (ctx, _bufs) = ctx_with(
            h.clone(),
            authed_config(),
            Arc::new(ScriptedPrompter::new()),
        );

        let a = CommentArgs {
            id: "7".to_owned(),
            body: Some("hi".to_owned()),
            body_file: None,
        };
        let err = run(&ctx, a).unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
        assert!(err.to_string().contains("issue #7 not found"), "msg: {err}");
    }

    #[test]
    fn comment_invalid_id_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        let (ctx, _bufs) = ctx_with(
            h.clone(),
            authed_config(),
            Arc::new(ScriptedPrompter::new()),
        );

        let a = CommentArgs {
            id: "not-a-number".to_owned(),
            body: Some("x".to_owned()),
            body_file: None,
        };
        let err = run(&ctx, a).unwrap_err();
        let flag = err.downcast_ref::<FlagError>().expect("FlagError");
        assert!(flag.to_string().contains("invalid issue id"), "{flag}");
    }

    #[test]
    fn comment_not_authed_returns_auth_error() {
        let h = Arc::new(FakeTransport::new());
        let (ctx, _bufs) = ctx_with(
            h.clone(),
            Arc::new(FileConfig::blank()),
            Arc::new(ScriptedPrompter::new()),
        );

        let a = CommentArgs {
            id: "7".to_owned(),
            body: Some("x".to_owned()),
            body_file: None,
        };
        let err = run(&ctx, a).unwrap_err();
        assert!(
            err.downcast_ref::<AuthError>().is_some(),
            "expected AuthError, got: {err:#}"
        );
    }
}
