//! `bb issue view` — show an issue's details, or open it in the browser.

use crate::api::BitbucketClient;
use crate::core::{AuthError, ColorScheme, Context, FlagError};
use clap::Args;

use crate::auth;

/// JSON fields an issue can be projected to with `--json`.
const FIELDS: &[&str] = &[
    "id", "title", "state", "kind", "priority", "content", "reporter", "links",
];

#[derive(Args, Debug)]
pub struct ViewArgs {
    /// Issue id
    #[arg(value_name = "ID")]
    pub id: String,
    /// Open the issue in the browser
    #[arg(long)]
    pub web: bool,
    #[command(flatten)]
    pub json: crate::output::JsonFlags,
}

/// Run `bb issue view`.
///
/// # Errors
/// Returns [`AuthError`] (exit 4) if not authenticated for the repo's host,
/// [`FlagError`] (exit 1) for a malformed id or when the issue is not found,
/// and propagates [`ApiError`](crate::core::ApiError) from the lookup.
pub fn run(ctx: &Context, args: ViewArgs) -> anyhow::Result<()> {
    let repo = ctx.base_repo()?;
    let host = repo.host().to_owned();

    let Some(header) = auth::header_for(ctx.config.as_ref(), &host) else {
        return Err(AuthError::new(host).into());
    };
    let client = BitbucketClient::new(ctx.transport.clone(), Some(header));

    // Issue ids are numeric; a non-numeric id would 404 on the API, so reject it
    // up front as a usage error.
    if args.id.parse::<u64>().is_err() {
        return Err(FlagError::new(format!("invalid issue id {:?}", args.id)).into());
    }

    let path = format!(
        "/repositories/{}/{}/issues/{}",
        repo.workspace(),
        repo.slug(),
        args.id,
    );

    let issue: crate::api::Issue = match client.get(&path) {
        Ok(issue) => issue,
        // 410 (Gone) means the tracker is disabled, not that this issue is
        // missing — distinguish it from a genuine 404.
        Err(e) if e.is_gone() => return Err(super::tracker_disabled(&repo).into()),
        Err(e) if e.is_not_found() => {
            return Err(FlagError::new(format!("issue #{} not found", args.id)).into());
        }
        Err(e) => return Err(e.into()),
    };

    if args.json.requested() {
        args.json.validate(FIELDS)?;
        args.json.emit(&ctx.io, serde_json::to_value(&issue)?)?;
        return Ok(());
    }

    if args.web {
        let url = issue
            .html_url()
            .ok_or_else(|| FlagError::new("no browser URL is available for this issue"))?;
        ctx.browser.browse(url)?;
        ctx.io.println(&format!("Opening {url} in your browser."));
        return Ok(());
    }

    let color = ctx.io.is_stdout_tty();
    ctx.io
        .print(&render_view(&issue, ctx.io.color_scheme(), color));
    Ok(())
}

/// Render a single issue's details. `color` gates state coloring (TTY only).
fn render_view(issue: &crate::api::Issue, cs: ColorScheme, color: bool) -> String {
    let title = issue.title.as_deref().unwrap_or("");
    let mut out = format!("#{} {}\n", issue.id, title);

    // state · kind · priority line (skip absent parts).
    let mut parts: Vec<String> = Vec::new();
    if let Some(state) = issue.state.as_deref() {
        parts.push(if color {
            color_state(cs, state)
        } else {
            state.to_owned()
        });
    }
    if let Some(kind) = issue.kind.as_deref() {
        parts.push(kind.to_owned());
    }
    if let Some(priority) = issue.priority.as_deref() {
        parts.push(priority.to_owned());
    }
    if !parts.is_empty() {
        out.push_str(&parts.join(" · "));
        out.push('\n');
    }

    if let Some(reporter) = &issue.reporter {
        out.push_str(&format!("Reporter: {}\n", reporter.label()));
    }

    out.push('\n');
    // Treat an empty or whitespace-only body as absent so we still show the
    // placeholder (the API may return `content.raw: ""`).
    let body = issue.body().map(str::trim).filter(|b| !b.is_empty());
    out.push_str(body.unwrap_or("No description provided."));
    out.push('\n');
    out
}

fn color_state(cs: ColorScheme, state: &str) -> String {
    match state {
        "new" | "open" => cs.green(state),
        "resolved" | "closed" => cs.cyan(state),
        "on_hold" => cs.yellow(state),
        "invalid" | "duplicate" | "wontfix" => cs.red(state),
        other => other.to_owned(),
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

    /// A git client that errors on every call — `repo_override` makes
    /// `base_repo()` skip git, so the tests never actually shell out.
    fn git() -> Arc<dyn GitClient> {
        Arc::new(ShellGit::new(Arc::new(StubRunner::new())))
    }

    fn config() -> Arc<dyn ConfigProvider> {
        let cfg = FileConfig::blank();
        cfg.set("bitbucket.org", "auth_type", "app_password")
            .unwrap();
        cfg.set("bitbucket.org", "username", "u").unwrap();
        cfg.set("bitbucket.org", "token", "t").unwrap();
        Arc::new(cfg)
    }

    fn args(id: &str, web: bool) -> ViewArgs {
        ViewArgs {
            id: id.to_owned(),
            web,
            json: crate::output::JsonFlags::default(),
        }
    }

    const ISSUE_42: &str = r#"{
        "id": 42,
        "title": "Add widget",
        "state": "new",
        "kind": "bug",
        "priority": "major",
        "reporter": {"display_name": "David"},
        "content": {"raw": "Implements the widget."},
        "links": {"html": {"href": "https://bitbucket.org/acme/widgets/issues/42"}}
    }"#;

    #[test]
    fn view_by_id_renders_details() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get issue 42",
            FakeTransport::rest(Method::Get, "/issues/42"),
            FakeTransport::json(200, ISSUE_42),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (mut ctx, bufs) = test_context(transport, git(), config(), prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        run(&ctx, args("42", false)).unwrap();

        let out = bufs.stdout_string();
        assert!(out.contains("#42 Add widget"), "out: {out}");
        assert!(out.contains("new"));
        assert!(out.contains("bug"));
        assert!(out.contains("major"));
        assert!(out.contains("Reporter: David"));
        assert!(out.contains("Implements the widget."));
    }

    #[test]
    fn view_missing_description_shows_placeholder() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get issue",
            FakeTransport::rest(Method::Get, "/issues/42"),
            FakeTransport::json(200, r#"{"id":42,"title":"T","state":"new","kind":"bug"}"#),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (mut ctx, bufs) = test_context(transport, git(), config(), prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        run(&ctx, args("42", false)).unwrap();
        assert!(bufs.stdout_string().contains("No description provided."));
    }

    #[test]
    fn view_blank_description_shows_placeholder() {
        // An empty / whitespace-only `content.raw` must still show the placeholder.
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get issue",
            FakeTransport::rest(Method::Get, "/issues/42"),
            FakeTransport::json(
                200,
                r#"{"id":42,"title":"T","state":"new","content":{"raw":"   \n  "}}"#,
            ),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (mut ctx, bufs) = test_context(transport, git(), config(), prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        run(&ctx, args("42", false)).unwrap();
        assert!(bufs.stdout_string().contains("No description provided."));
    }

    #[test]
    fn view_web_browses_and_prints_url() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get issue 42",
            FakeTransport::rest(Method::Get, "/issues/42"),
            FakeTransport::json(200, ISSUE_42),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (mut ctx, bufs) = test_context(transport, git(), config(), prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        run(&ctx, args("42", true)).unwrap();

        let out = bufs.stdout_string();
        assert!(
            out.contains("https://bitbucket.org/acme/widgets/issues/42"),
            "out: {out}"
        );
        // --web must not render the body.
        assert!(!out.contains("Implements the widget."));
    }

    #[test]
    fn view_not_found_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get issue 404",
            FakeTransport::rest(Method::Get, "/issues/99"),
            FakeTransport::json(
                404,
                r#"{"type":"error","error":{"message":"No such issue."}}"#,
            ),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (mut ctx, _bufs) = test_context(transport, git(), config(), prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        let err = run(&ctx, args("99", false)).unwrap_err();
        let flag = err.downcast_ref::<FlagError>();
        assert!(flag.is_some(), "expected FlagError, got: {err}");
        assert!(
            err.to_string().contains("issue #99 not found"),
            "msg: {err}"
        );
    }

    #[test]
    fn view_tracker_disabled_410_reports_tracker() {
        // #77: 410 = disabled tracker, distinct from a missing issue (404).
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get issue 410",
            FakeTransport::rest(Method::Get, "/issues/99"),
            FakeTransport::json(410, r#"{"type":"error","error":{"message":"Gone"}}"#),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (mut ctx, _bufs) = test_context(transport, git(), config(), prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        let err = run(&ctx, args("99", false)).unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
        assert!(
            err.to_string().contains("issue tracker is not enabled"),
            "should report disabled tracker, not 'not found': {err}"
        );
    }

    #[test]
    fn view_json_emits_projected_fields() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get issue 42 json",
            FakeTransport::rest(Method::Get, "/issues/42"),
            FakeTransport::json(200, ISSUE_42),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (mut ctx, bufs) = test_context(transport, git(), config(), prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        let a = ViewArgs {
            id: "42".to_owned(),
            web: false,
            json: crate::output::JsonFlags {
                json: vec!["id".into(), "title".into(), "state".into()],
                jq: None,
                template: None,
            },
        };
        run(&ctx, a).unwrap();

        let out = bufs.stdout_string();
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(v["id"], 42);
        assert_eq!(v["title"], "Add widget");
        assert_eq!(v["state"], "new");
        // Unrequested fields are projected away.
        assert!(v.get("reporter").is_none(), "out: {out}");
    }

    #[test]
    fn view_json_takes_precedence_over_web() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get issue 42 json web",
            FakeTransport::rest(Method::Get, "/issues/42"),
            FakeTransport::json(200, ISSUE_42),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (mut ctx, bufs) = test_context(transport, git(), config(), prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        let a = ViewArgs {
            id: "42".to_owned(),
            web: true,
            json: crate::output::JsonFlags {
                json: vec!["id".into()],
                jq: None,
                template: None,
            },
        };
        run(&ctx, a).unwrap();

        let out = bufs.stdout_string();
        // --json wins: JSON is emitted, no browser-open message.
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(v["id"], 42);
        assert!(!out.contains("Opening"), "out: {out}");
    }

    #[test]
    fn view_json_unknown_field_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get issue 42 json bogus",
            FakeTransport::rest(Method::Get, "/issues/42"),
            FakeTransport::json(200, ISSUE_42),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (mut ctx, _bufs) = test_context(transport, git(), config(), prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        let a = ViewArgs {
            id: "42".to_owned(),
            web: false,
            json: crate::output::JsonFlags {
                json: vec!["bogus".into()],
                jq: None,
                template: None,
            },
        };
        let err = run(&ctx, a).unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some());
    }

    #[test]
    fn view_invalid_id_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (mut ctx, _bufs) = test_context(transport, git(), config(), prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        let err = run(&ctx, args("not-a-number", false)).unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some());
    }

    #[test]
    fn view_not_logged_in_is_auth_error() {
        let h = Arc::new(FakeTransport::new());
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let cfg: Arc<dyn ConfigProvider> = Arc::new(FileConfig::blank());
        let (mut ctx, _bufs) = test_context(transport, git(), cfg, prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        let err = run(&ctx, args("42", false)).unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some());
    }

    #[test]
    fn render_view_colors_state_when_enabled() {
        let (mut io, _) = crate::core::IoStreams::test();
        io.set_stdout_tty(true);
        let cs = io.color_scheme();
        let issue: crate::api::Issue = serde_json::from_str(ISSUE_42).unwrap();
        let out = render_view(&issue, cs, true);
        // `new` must use the green code, not red.
        assert!(out.contains("new"));
        assert!(!out.contains("[31m"), "new must not use the red code");
    }
}
