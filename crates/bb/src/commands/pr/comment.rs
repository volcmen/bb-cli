//! `bb pr comment` — add or list comments on a pull request.

use crate::api::models::{Rendered, User};
use crate::api::BitbucketClient;
use crate::core::{AuthError, Context, FlagError};
use clap::Args;

#[derive(Args, Debug)]
pub struct CommentArgs {
    /// Pull request id (defaults to the PR for the current branch)
    #[arg(value_name = "ID")]
    pub id: Option<String>,
    /// Comment body
    #[arg(long, short = 'b')]
    pub body: Option<String>,
    /// Read the comment body from a file (use "-" for stdin)
    #[arg(long = "body-file", short = 'F', value_name = "FILE")]
    pub body_file: Option<String>,
    /// List existing comments instead of adding one
    #[arg(long)]
    pub list: bool,
}

// ----- request/response shapes ------------------------------------------

#[derive(serde::Serialize)]
struct Content<'a> {
    raw: &'a str,
}

#[derive(serde::Serialize)]
struct CommentBody<'a> {
    content: Content<'a>,
}

/// One comment in a `GET .../comments` page.
#[derive(serde::Deserialize)]
struct PrComment {
    #[serde(default)]
    content: Rendered,
    user: Option<User>,
    #[serde(default)]
    deleted: bool,
}

/// Run `bb pr comment`.
///
/// # Errors
/// Returns [`AuthError`] (exit 4) when not authenticated, [`FlagError`] (exit 1)
/// for a bad id / missing body / PR not found, and propagates
/// [`ApiError`](crate::core::ApiError) / IO errors.
pub fn run(ctx: &Context, args: CommentArgs) -> anyhow::Result<()> {
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
    let path = format!(
        "/repositories/{}/{}/pullrequests/{id}/comments",
        repo.workspace(),
        repo.slug()
    );

    if args.list {
        return list_comments(ctx, &client, &path, id);
    }

    let body = resolve_body(ctx, &args)?;
    let payload = CommentBody {
        content: Content { raw: &body },
    };
    let _resp: serde_json::Value = client.post(&path, &payload)?;
    ctx.io
        .println(&format!("✓ Commented on pull request #{id}"));
    Ok(())
}

/// Print every non-deleted comment as `@{author}:` followed by its raw body.
fn list_comments(
    ctx: &Context,
    client: &BitbucketClient,
    path: &str,
    id: u64,
) -> anyhow::Result<()> {
    let comments: Vec<PrComment> = client.paginate(path, None)?;
    let live: Vec<&PrComment> = comments.iter().filter(|c| !c.deleted).collect();
    if live.is_empty() {
        ctx.io
            .println(&format!("No comments on pull request #{id}."));
        return Ok(());
    }
    for c in live {
        let author = c
            .user
            .as_ref()
            .and_then(|u| u.display_name.as_deref())
            .unwrap_or("unknown");
        let raw = c.content.raw.as_deref().unwrap_or("");
        ctx.io.println(&format!("@{author}:"));
        ctx.io.println(raw);
        ctx.io.println("");
    }
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

    fn git() -> Arc<dyn GitClient> {
        Arc::new(ShellGit::new(Arc::new(StubRunner::new())))
    }

    fn ctx_with(http: Arc<FakeTransport>) -> (Context, crate::core::TestBuffers) {
        let transport: Arc<dyn Transport> = http;
        let (mut ctx, bufs) = test_context(
            transport,
            git(),
            authed_config(),
            Arc::new(ScriptedPrompter::new()),
            false,
        );
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));
        (ctx, bufs)
    }

    fn args() -> CommentArgs {
        CommentArgs {
            id: Some("42".to_owned()),
            body: None,
            body_file: None,
            list: false,
        }
    }

    #[test]
    fn comment_posts_content() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "post comment",
            FakeTransport::rest(Method::Post, "/pullrequests/42/comments"),
            FakeTransport::json(201, r#"{"id": 1, "content": {"raw": "hi"}}"#),
        );
        let (ctx, bufs) = ctx_with(h.clone());

        let a = CommentArgs {
            body: Some("hi".to_owned()),
            ..args()
        };
        run(&ctx, a).unwrap();

        let reqs = h.requests.lock().unwrap();
        let post = reqs.iter().find(|r| r.method == Method::Post).unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(post.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["content"]["raw"], "hi");
        assert!(bufs
            .stdout_string()
            .contains("✓ Commented on pull request #42"));
    }

    #[test]
    fn comment_body_file_reads_file() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "post comment",
            FakeTransport::rest(Method::Post, "/pullrequests/42/comments"),
            FakeTransport::json(201, r#"{"id": 1}"#),
        );
        let (ctx, _bufs) = ctx_with(h.clone());

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("c.md");
        std::fs::write(&file, "from file").unwrap();

        let a = CommentArgs {
            body_file: Some(file.to_string_lossy().into_owned()),
            ..args()
        };
        run(&ctx, a).unwrap();

        let reqs = h.requests.lock().unwrap();
        let post = reqs.iter().find(|r| r.method == Method::Post).unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(post.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["content"]["raw"], "from file");
    }

    #[test]
    fn comment_list_renders_live_comments_only() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list comments",
            FakeTransport::rest(Method::Get, "/pullrequests/42/comments"),
            FakeTransport::json(
                200,
                r#"{"values":[
                    {"content":{"raw":"looks good"},"user":{"display_name":"Dana"}},
                    {"content":{"raw":"gone"},"user":{"display_name":"Ghost"},"deleted":true}
                ]}"#,
            ),
        );
        let (ctx, bufs) = ctx_with(h.clone());

        let a = CommentArgs {
            list: true,
            ..args()
        };
        run(&ctx, a).unwrap();

        let out = bufs.stdout_string();
        assert!(out.contains("@Dana:"), "out: {out}");
        assert!(out.contains("looks good"), "out: {out}");
        // Deleted comment is skipped.
        assert!(!out.contains("gone"), "out: {out}");
    }

    #[test]
    fn comment_no_body_non_interactive_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        let (ctx, _bufs) = ctx_with(h.clone());

        let err = run(&ctx, args()).unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
        assert_eq!(h.request_count(), 0);
    }

    #[test]
    fn comment_not_authed_is_auth_error() {
        let h = Arc::new(FakeTransport::new());
        let transport: Arc<dyn Transport> = h.clone();
        let (mut ctx, _bufs) = test_context(
            transport,
            git(),
            Arc::new(FileConfig::blank()),
            Arc::new(ScriptedPrompter::new()),
            false,
        );
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        let a = CommentArgs {
            body: Some("x".to_owned()),
            ..args()
        };
        let err = run(&ctx, a).unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some(), "got: {err}");
    }
}
