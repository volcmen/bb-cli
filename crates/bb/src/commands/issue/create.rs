//! `bb issue create`.

use bb_api::{BitbucketClient, Issue};
use bb_core::{AuthError, Context, FlagError};
use clap::Args;

#[derive(Args, Debug)]
pub struct CreateArgs {
    /// Issue title
    #[arg(long, short)]
    pub title: Option<String>,
    /// Issue body/content
    #[arg(long, short)]
    pub body: Option<String>,
    /// Read the body from a file (use "-" for stdin)
    #[arg(long = "body-file", short = 'F', value_name = "FILE")]
    pub body_file: Option<String>,
    /// Issue kind
    #[arg(long, value_parser = ["bug", "enhancement", "proposal", "task"])]
    pub kind: Option<String>,
    /// Issue priority
    #[arg(long, value_parser = ["trivial", "minor", "major", "critical", "blocker"])]
    pub priority: Option<String>,
}

// ----- request body shapes ----------------------------------------------

/// A `{ "raw": ... }` content wrapper, matching Bitbucket's rendered-content shape.
#[derive(serde::Serialize)]
struct Content<'a> {
    raw: &'a str,
}

/// The JSON body sent to `POST .../issues`.
#[derive(serde::Serialize)]
struct CreateIssueBody<'a> {
    title: &'a str,
    content: Content<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    priority: Option<&'a str>,
}

/// Run `bb issue create`.
///
/// # Errors
/// Returns [`AuthError`] (exit 4) when not authenticated, [`FlagError`] for
/// usage errors (e.g. no title when non-interactive), and propagates
/// [`ApiError`](bb_core::ApiError) / IO errors.
pub fn run(ctx: &Context, args: CreateArgs) -> anyhow::Result<()> {
    let repo = ctx.base_repo()?;
    let host = repo.host().to_owned();

    let Some(header) = crate::auth::header_for(ctx.config.as_ref(), &host) else {
        return Err(AuthError::new(host).into());
    };
    let client = BitbucketClient::new(ctx.transport.clone(), Some(header));

    // title: --title, else prompt when interactive, else FlagError.
    let title = match &args.title {
        Some(t) => t.clone(),
        None => {
            if ctx.io.can_prompt() {
                ctx.prompter.input("Title", None).map_err(to_anyhow)?
            } else {
                return Err(
                    FlagError::new("--title required when not running interactively").into(),
                );
            }
        }
    };

    // body: --body, else --body-file (`-` => stdin), else "".
    let body = resolve_body(ctx, &args)?;

    let payload = CreateIssueBody {
        title: &title,
        content: Content { raw: &body },
        kind: args.kind.as_deref(),
        priority: args.priority.as_deref(),
    };

    let path = format!("/repositories/{}/{}/issues", repo.workspace(), repo.slug());
    let issue: Issue = client.post(&path, &payload)?;

    ctx.io.println(&format!("✓ Created issue #{}", issue.id));
    if let Some(url) = issue.html_url() {
        ctx.io.println(url);
    }
    Ok(())
}

/// Resolve the issue body from `--body`, then `--body-file` (`-` => stdin),
/// else the empty string.
fn resolve_body(ctx: &Context, args: &CreateArgs) -> anyhow::Result<String> {
    if let Some(b) = &args.body {
        return Ok(b.clone());
    }
    if let Some(file) = &args.body_file {
        if file == "-" {
            return Ok(ctx.io.read_stdin_to_string()?);
        }
        return Ok(std::fs::read_to_string(file)?);
    }
    Ok(String::new())
}

fn to_anyhow(err: bb_core::PromptError) -> anyhow::Error {
    match err {
        bb_core::PromptError::Cancelled => bb_core::CancelError.into(),
        other => anyhow::anyhow!(other),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bb_api::testing::FakeTransport;
    use bb_config::FileConfig;
    use bb_core::{ConfigProvider, GitClient, IoStreams, Method, Prompter, RepoId, Transport};
    use bb_git::{ShellGit, StubRunner};

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
    ) -> (Context, bb_core::TestBuffers) {
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
    ) -> (Context, bb_core::TestBuffers) {
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

    fn args() -> CreateArgs {
        CreateArgs {
            title: None,
            body: None,
            body_file: None,
            kind: None,
            priority: None,
        }
    }

    #[test]
    fn create_happy_path_posts_payload_and_prints_url() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "create issue",
            FakeTransport::rest(Method::Post, "/repositories/acme/widgets/issues"),
            FakeTransport::json(
                201,
                r#"{"id": 7, "title": "Bug report", "kind": "bug",
                    "content": {"raw": "it broke"},
                    "links": {"html": {"href": "https://bitbucket.org/acme/widgets/issues/7"}}}"#,
            ),
        );
        let (ctx, bufs) = ctx_with(
            h.clone(),
            authed_config(),
            Arc::new(ScriptedPrompter::new()),
        );

        let a = CreateArgs {
            title: Some("Bug report".to_owned()),
            body: Some("it broke".to_owned()),
            kind: Some("bug".to_owned()),
            ..args()
        };
        run(&ctx, a).unwrap();

        // Printed confirmation + URL.
        let out = bufs.stdout_string();
        assert!(out.contains("✓ Created issue #7"), "out: {out}");
        assert!(
            out.contains("https://bitbucket.org/acme/widgets/issues/7"),
            "out: {out}"
        );

        // POST body shape.
        let reqs = h.requests.lock().unwrap();
        let post = reqs
            .iter()
            .find(|r| r.method == Method::Post)
            .expect("a POST");
        let body: serde_json::Value =
            serde_json::from_slice(post.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["title"], "Bug report");
        assert_eq!(body["content"]["raw"], "it broke");
        assert_eq!(body["kind"], "bug");
        // priority omitted (skip_serializing_if).
        assert!(body.get("priority").is_none());
    }

    #[test]
    fn create_prompts_for_title_when_interactive() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "create issue",
            FakeTransport::rest(Method::Post, "/repositories/acme/widgets/issues"),
            FakeTransport::json(201, r#"{"id": 9}"#),
        );
        let prompter = Arc::new(ScriptedPrompter::new().input("Typed title"));
        let (ctx, bufs) = interactive_ctx(h.clone(), authed_config(), prompter);

        run(&ctx, args()).unwrap();

        assert!(bufs.stdout_string().contains("✓ Created issue #9"));
        let reqs = h.requests.lock().unwrap();
        let post = reqs.iter().find(|r| r.method == Method::Post).unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(post.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["title"], "Typed title");
        assert_eq!(body["content"]["raw"], "");
    }

    #[test]
    fn create_non_interactive_missing_title_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        let (ctx, _bufs) = ctx_with(
            h.clone(),
            authed_config(),
            Arc::new(ScriptedPrompter::new()),
        );

        let err = run(&ctx, args()).unwrap_err();
        let flag = err.downcast_ref::<FlagError>().expect("FlagError");
        assert!(flag.to_string().contains("--title required"), "{flag}");
    }

    #[test]
    fn create_not_authed_returns_auth_error() {
        let h = Arc::new(FakeTransport::new());
        let (ctx, _bufs) = ctx_with(
            h.clone(),
            Arc::new(FileConfig::blank()),
            Arc::new(ScriptedPrompter::new()),
        );

        let a = CreateArgs {
            title: Some("T".to_owned()),
            ..args()
        };
        let err = run(&ctx, a).unwrap_err();
        assert!(
            err.downcast_ref::<AuthError>().is_some(),
            "expected AuthError, got: {err:#}"
        );
    }
}
