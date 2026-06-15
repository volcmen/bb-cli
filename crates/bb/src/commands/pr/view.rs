//! `bb pr view` — show a pull request's details, or open it in the browser.

use bb_api::BitbucketClient;
use bb_core::{AuthError, ColorScheme, Context};
use clap::Args;

use super::finder;
use crate::auth;

#[derive(Args, Debug)]
pub struct ViewArgs {
    /// Pull request id (defaults to the PR for the current branch)
    #[arg(value_name = "ID")]
    pub id: Option<String>,
    /// Open the pull request in the browser
    #[arg(long)]
    pub web: bool,
}

/// Run `bb pr view`.
///
/// # Errors
/// Returns [`AuthError`] (exit 4) if not authenticated for the repo's host,
/// [`FlagError`](bb_core::FlagError) for a malformed id, and propagates
/// [`ApiError`](bb_core::ApiError) from the lookup.
pub fn run(ctx: &Context, args: ViewArgs) -> anyhow::Result<()> {
    let repo = ctx.base_repo()?;
    let host = repo.host().to_owned();

    let Some(header) = auth::header_for(ctx.config.as_ref(), &host) else {
        return Err(AuthError::new(host).into());
    };
    let client = BitbucketClient::new(ctx.transport.clone(), Some(header));

    let pr = finder::resolve(ctx, &client, &repo, args.id.as_deref())?;

    if args.web {
        match pr.html_url() {
            Some(url) => {
                ctx.browser.browse(url)?;
                ctx.io.println(&format!("Opening {url} in your browser."));
            }
            None => ctx
                .io
                .println("no browser URL is available for this pull request"),
        }
        return Ok(());
    }

    let color = ctx.io.is_stdout_tty();
    ctx.io
        .print(&render_view(&pr, ctx.io.color_scheme(), color));
    Ok(())
}

/// Render a single PR's details. `color` gates state coloring (TTY only).
fn render_view(pr: &bb_api::PullRequest, cs: ColorScheme, color: bool) -> String {
    let title = pr.title.as_deref().unwrap_or("");
    let mut out = format!("#{} {}\n", pr.id, title);

    if let Some(state) = pr.state.as_deref() {
        let rendered = if color {
            color_state(cs, state)
        } else {
            state.to_owned()
        };
        out.push_str(&format!("{rendered}\n"));
    }

    if let Some(author) = &pr.author {
        out.push_str(&format!("Author: {}\n", author.label()));
    }

    out.push_str(&format!(
        "{} → {}\n",
        pr.source.branch_name(),
        pr.destination.branch_name()
    ));

    out.push('\n');
    // Treat an empty or whitespace-only body as absent so we still show the
    // placeholder (the API may return `description: ""`).
    let body = pr.body().map(str::trim).filter(|b| !b.is_empty());
    out.push_str(body.unwrap_or("No description provided."));
    out.push('\n');
    out
}

fn color_state(cs: ColorScheme, state: &str) -> String {
    match state {
        "OPEN" => cs.green(state),
        // No magenta in the scheme; cyan is the project convention for MERGED.
        "MERGED" => cs.cyan(state),
        "DECLINED" | "SUPERSEDED" => cs.red(state),
        other => cs.gray(other),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bb_api::testing::FakeTransport;
    use bb_config::FileConfig;
    use bb_core::{ConfigProvider, GitClient, Method, Transport};
    use bb_git::{ShellGit, StubRunner};

    use super::*;
    use crate::testsupport::{test_context, ScriptedPrompter};

    /// Git stub that answers `remote -v` (for `base_repo`) and nothing else.
    fn git() -> Arc<dyn GitClient> {
        let s = Arc::new(StubRunner::new());
        s.register(
            "remote -v",
            0,
            "origin\tgit@bitbucket.org:acme/widgets.git (fetch)\n\
             origin\tgit@bitbucket.org:acme/widgets.git (push)\n",
        );
        Arc::new(ShellGit::new(s))
    }

    /// Git stub answering `remote -v` and the current-branch query.
    fn git_with_branch(branch: &str) -> Arc<dyn GitClient> {
        let s = Arc::new(StubRunner::new());
        s.register(
            "remote -v",
            0,
            "origin\tgit@bitbucket.org:acme/widgets.git (fetch)\n\
             origin\tgit@bitbucket.org:acme/widgets.git (push)\n",
        );
        s.register("rev-parse --abbrev-ref HEAD", 0, &format!("{branch}\n"));
        Arc::new(ShellGit::new(s))
    }

    fn config() -> Arc<dyn ConfigProvider> {
        let cfg = FileConfig::blank();
        cfg.set("bitbucket.org", "auth_type", "app_password")
            .unwrap();
        cfg.set("bitbucket.org", "username", "u").unwrap();
        cfg.set("bitbucket.org", "token", "t").unwrap();
        Arc::new(cfg)
    }

    fn args(id: Option<&str>, web: bool) -> ViewArgs {
        ViewArgs {
            id: id.map(ToOwned::to_owned),
            web,
        }
    }

    const PR_42: &str = r#"{
        "id": 42,
        "title": "Add widget",
        "state": "OPEN",
        "source": {"branch": {"name": "feature/x"}},
        "destination": {"branch": {"name": "main"}},
        "author": {"display_name": "David"},
        "description": "Implements the widget.",
        "links": {"html": {"href": "https://bitbucket.org/acme/widgets/pull-requests/42"}}
    }"#;

    #[test]
    fn view_by_id_renders_details() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get pr 42",
            FakeTransport::rest(Method::Get, "/pullrequests/42"),
            FakeTransport::json(200, PR_42),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, bufs) = test_context(transport, git(), config(), prompter, false);

        run(&ctx, args(Some("42"), false)).unwrap();

        let out = bufs.stdout_string();
        assert!(out.contains("#42 Add widget"), "out: {out}");
        assert!(out.contains("OPEN"));
        assert!(out.contains("David"));
        assert!(out.contains("feature/x → main"));
        assert!(out.contains("Implements the widget."));
    }

    #[test]
    fn view_missing_description_shows_placeholder() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get pr",
            FakeTransport::rest(Method::Get, "/pullrequests/42"),
            FakeTransport::json(
                200,
                r#"{"id":42,"title":"T","state":"OPEN",
                    "source":{"branch":{"name":"s"}},
                    "destination":{"branch":{"name":"d"}}}"#,
            ),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, bufs) = test_context(transport, git(), config(), prompter, false);

        run(&ctx, args(Some("42"), false)).unwrap();
        assert!(bufs.stdout_string().contains("No description provided."));
    }

    #[test]
    fn view_blank_description_shows_placeholder() {
        // An empty / whitespace-only `description` must still show the placeholder
        // (PullRequest::body() returns Some("") here, not None).
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get pr",
            FakeTransport::rest(Method::Get, "/pullrequests/42"),
            FakeTransport::json(
                200,
                r#"{"id":42,"title":"T","state":"OPEN",
                    "source":{"branch":{"name":"s"}},
                    "destination":{"branch":{"name":"d"}},
                    "description":"   \n  "}"#,
            ),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, bufs) = test_context(transport, git(), config(), prompter, false);

        run(&ctx, args(Some("42"), false)).unwrap();
        assert!(bufs.stdout_string().contains("No description provided."));
    }

    #[test]
    fn view_web_browses_and_prints_url() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get pr 42",
            FakeTransport::rest(Method::Get, "/pullrequests/42"),
            FakeTransport::json(200, PR_42),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, bufs) = test_context(transport, git(), config(), prompter, false);

        run(&ctx, args(Some("42"), true)).unwrap();

        // RecordingBrowser is internal to test_context; assert via the printed URL.
        let out = bufs.stdout_string();
        assert!(
            out.contains("https://bitbucket.org/acme/widgets/pull-requests/42"),
            "out: {out}"
        );
        // --web must not render the body.
        assert!(!out.contains("Implements the widget."));
    }

    #[test]
    fn view_resolves_by_current_branch() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list by branch",
            FakeTransport::rest(Method::Get, "/pullrequests?state=OPEN&q="),
            FakeTransport::json(
                200,
                r#"{"values":[{"id":7,"title":"Branch PR","state":"OPEN",
                    "source":{"branch":{"name":"feature/x"}},
                    "destination":{"branch":{"name":"main"}}}]}"#,
            ),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, bufs) = test_context(
            transport,
            git_with_branch("feature/x"),
            config(),
            prompter,
            false,
        );

        run(&ctx, args(None, false)).unwrap();
        assert!(bufs.stdout_string().contains("#7 Branch PR"));
    }

    #[test]
    fn view_not_logged_in_is_auth_error() {
        let h = Arc::new(FakeTransport::new());
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let cfg: Arc<dyn ConfigProvider> = Arc::new(FileConfig::blank());
        let (ctx, _bufs) = test_context(transport, git(), cfg, prompter, false);

        let err = run(&ctx, args(Some("42"), false)).unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some());
    }

    #[test]
    fn view_invalid_id_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, _bufs) = test_context(transport, git(), config(), prompter, false);

        let err = run(&ctx, args(Some("not-a-number"), false)).unwrap_err();
        assert!(err.downcast_ref::<bb_core::FlagError>().is_some());
    }

    #[test]
    fn render_view_colors_state_when_enabled() {
        let (mut io, _) = bb_core::IoStreams::test();
        io.set_stdout_tty(true);
        let cs = io.color_scheme();
        let pr: bb_api::PullRequest = serde_json::from_str(PR_42).unwrap();
        let out = render_view(&pr, cs, true);
        // OPEN must use the green code, not red.
        assert!(out.contains("OPEN"));
        assert!(!out.contains("[31m"), "OPEN must not use the red code");
    }
}
