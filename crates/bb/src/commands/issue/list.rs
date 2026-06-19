//! `bb issue list` — list issues for the current repository's tracker.

use crate::api::models::Issue;
use crate::api::BitbucketClient;
use crate::core::{AuthError, ColorScheme, Context};
use clap::Args;

use crate::auth;
use crate::render::{pad, percent_encode, sanitize};

/// JSON fields an issue can be projected to with `--json`.
const FIELDS: &[&str] = &[
    "id", "title", "state", "kind", "priority", "content", "reporter", "links",
];

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Filter by state (new, open, resolved, closed, on_hold, invalid, duplicate, wontfix)
    #[arg(long)]
    pub state: Option<String>,
    /// Maximum number of issues to list
    #[arg(long, short = 'L', default_value_t = 30)]
    pub limit: usize,
    #[command(flatten)]
    pub json: crate::output::JsonFlags,
}

/// Run `bb issue list`.
///
/// # Errors
/// Returns [`AuthError`] (exit 4) if not authenticated for the repo's host, and
/// [`FlagError`](crate::core::FlagError) (exit 1) if the repo's issue tracker is
/// disabled (410 Gone, or a 404 whose body says the repo has no issue tracker)
/// or if the repository doesn't exist / isn't accessible (a plain 404). Other
/// [`ApiError`](crate::core::ApiError)s from the listing call are propagated.
pub fn run(ctx: &Context, args: ListArgs) -> anyhow::Result<()> {
    let repo = ctx.base_repo()?;
    let host = repo.host().to_owned();

    let Some(header) = auth::header_for(ctx.config.as_ref(), &host) else {
        return Err(AuthError::new(host).into());
    };
    let client = BitbucketClient::new(ctx.transport.clone(), Some(header));

    // Bitbucket caps pagelen at 50; never request more than the user wants.
    let pagelen = args.limit.clamp(1, 50);
    let mut path = format!(
        "/repositories/{}/{}/issues?sort=-updated_on&pagelen={pagelen}",
        repo.workspace(),
        repo.slug(),
    );
    if let Some(state) = &args.state {
        path.push_str(&format!(
            "&q={}",
            percent_encode(&format!("state=\"{state}\""))
        ));
    }

    let issues: Vec<Issue> = match client.paginate(&path, Some(args.limit)) {
        Ok(issues) => issues,
        // A disabled tracker returns 410 (Gone) on `/issues`. A 404 is
        // ambiguous: it's a disabled tracker only when the body says so —
        // otherwise the repository is missing or inaccessible.
        Err(e) if e.is_gone() => return Err(super::tracker_disabled(&repo).into()),
        Err(e) if e.is_not_found() => return Err(super::repo_level_404(&repo, &e).into()),
        Err(e) => return Err(e.into()),
    };

    if args.json.requested() {
        args.json.validate(FIELDS)?;
        args.json.emit(&ctx.io, serde_json::to_value(&issues)?)?;
        return Ok(());
    }

    if issues.is_empty() {
        ctx.io.println(&format!(
            "No issues match your search in {}/{}.",
            repo.workspace(),
            repo.slug()
        ));
        return Ok(());
    }

    if ctx.io.is_stdout_tty() {
        ctx.io.print(&render_table(&issues, ctx.io.color_scheme()));
    } else {
        ctx.io.print(&render_tsv(&issues));
    }
    Ok(())
}

/// Render a list of issues for a TTY: a header row plus aligned, colored columns.
fn render_table(issues: &[Issue], cs: ColorScheme) -> String {
    // Plain (uncolored) cell text, used for width computation.
    let rows: Vec<[String; 4]> = issues
        .iter()
        .map(|i| {
            [
                format!("#{}", i.id),
                sanitize(i.title.as_deref().unwrap_or_default()),
                i.state.clone().unwrap_or_default(),
                i.kind.clone().unwrap_or_default(),
            ]
        })
        .collect();

    let headers = ["ID", "TITLE", "STATE", "KIND"];
    let mut widths = headers.map(str::len);
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }

    let mut out = String::new();
    // Header (bold), padded by plain width.
    for (i, h) in headers.iter().enumerate() {
        out.push_str(&pad(&cs.bold(h), h.chars().count(), widths[i]));
        if i + 1 < headers.len() {
            out.push_str("  ");
        }
    }
    out.push('\n');

    for row in &rows {
        // id (cyan), title (plain), state (colored by state), kind (plain)
        let id = cs.cyan(&row[0]);
        let state = color_state(cs, &row[2]);
        let cells = [id, row[1].clone(), state, row[3].clone()];
        let plain_lens = [
            row[0].chars().count(),
            row[1].chars().count(),
            row[2].chars().count(),
            row[3].chars().count(),
        ];
        for (i, cell) in cells.iter().enumerate() {
            out.push_str(&pad(cell, plain_lens[i], widths[i]));
            if i + 1 < cells.len() {
                out.push_str("  ");
            }
        }
        out.push('\n');
    }
    out
}

/// Render a list of issues for a pipe/script: one tab-separated line per issue,
/// no color and no header.
fn render_tsv(issues: &[Issue]) -> String {
    let mut out = String::new();
    for i in issues {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            i.id,
            sanitize(i.title.as_deref().unwrap_or_default()),
            i.state.as_deref().unwrap_or_default().to_owned(),
            i.kind.as_deref().unwrap_or_default().to_owned(),
        ));
    }
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
    use crate::core::{ConfigProvider, FlagError, GitClient, Method, RepoId, Transport};
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
        cfg.set("bitbucket.org", "username", "davidd").unwrap();
        cfg.set("bitbucket.org", "token", "secret").unwrap();
        Arc::new(cfg)
    }

    fn list_args() -> ListArgs {
        ListArgs {
            state: None,
            limit: 30,
            json: crate::output::JsonFlags::default(),
        }
    }

    const TWO_ISSUES: &str = r#"{
        "values": [
            {"id": 7, "title": "Fix bug", "state": "new", "kind": "bug"},
            {"id": 9, "title": "Add feature", "state": "open", "kind": "enhancement"}
        ]
    }"#;

    #[test]
    fn list_tsv_when_not_tty() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list",
            FakeTransport::rest(Method::Get, "/issues"),
            FakeTransport::json(200, TWO_ISSUES),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (mut ctx, bufs) = test_context(transport, git(), config(), prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        run(&ctx, list_args()).unwrap();

        let out = bufs.stdout_string();
        assert_eq!(
            out,
            "7\tFix bug\tnew\tbug\n9\tAdd feature\topen\tenhancement\n"
        );
        assert!(!out.contains("ID"));
    }

    #[test]
    fn list_table_when_tty() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list",
            FakeTransport::rest(Method::Get, "/issues"),
            FakeTransport::json(200, TWO_ISSUES),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (mut ctx, bufs) = test_context(transport, git(), config(), prompter, true);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        run(&ctx, list_args()).unwrap();

        let out = bufs.stdout_string();
        let first = out.lines().next().unwrap();
        assert!(first.contains("ID"));
        assert!(first.contains("TITLE"));
        assert!(first.contains("STATE"));
        assert!(first.contains("KIND"));
        assert!(out.contains("#7"));
        assert!(out.contains("#9"));
    }

    #[test]
    fn list_default_sorts_by_updated() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list sort",
            FakeTransport::rest(Method::Get, "/issues"),
            FakeTransport::json(200, r#"{"values": []}"#),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (mut ctx, _bufs) = test_context(transport, git(), config(), prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        run(&ctx, list_args()).unwrap();

        let reqs = h.requests.lock().unwrap();
        let url = &reqs[0].url;
        assert!(url.contains("sort=-updated_on"), "url: {url}");
        // No state filter ⇒ no q= clause.
        assert!(!url.contains("q="), "url: {url}");
    }

    #[test]
    fn list_state_adds_query_filter() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list state",
            FakeTransport::rest(Method::Get, "/issues"),
            FakeTransport::json(200, r#"{"values": []}"#),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (mut ctx, _bufs) = test_context(transport, git(), config(), prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        let a = ListArgs {
            state: Some("open".to_owned()),
            ..list_args()
        };
        run(&ctx, a).unwrap();

        let reqs = h.requests.lock().unwrap();
        let url = &reqs[0].url;
        assert!(url.contains("q="), "url: {url}");
        // state="open" must be url-encoded (quotes → %22).
        assert!(url.contains("state%3D%22open%22"), "url: {url}");
    }

    #[test]
    fn list_pagelen_clamped_to_limit() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list small limit",
            FakeTransport::rest(Method::Get, "/issues"),
            FakeTransport::json(200, r#"{"values": []}"#),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (mut ctx, _bufs) = test_context(transport, git(), config(), prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        let a = ListArgs {
            limit: 5,
            ..list_args()
        };
        run(&ctx, a).unwrap();

        let reqs = h.requests.lock().unwrap();
        let url = &reqs[0].url;
        // limit (5) < 50, so pagelen must be 5, not 50.
        assert!(url.contains("pagelen=5"), "url: {url}");
    }

    #[test]
    fn list_empty_prints_message() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list empty",
            FakeTransport::rest(Method::Get, "/issues"),
            FakeTransport::json(200, r#"{"values": []}"#),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (mut ctx, bufs) = test_context(transport, git(), config(), prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        run(&ctx, list_args()).unwrap();
        assert!(bufs
            .stdout_string()
            .contains("No issues match your search in acme/widgets."));
    }

    #[test]
    fn list_tracker_disabled_404_body_is_flag_error() {
        // A 404 whose body says the repo has no issue tracker => tracker-disabled
        // (not a missing repo).
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list 404 no tracker",
            FakeTransport::rest(Method::Get, "/issues"),
            FakeTransport::json(
                404,
                r#"{"type":"error","error":{"message":"Repository has no issue tracker."}}"#,
            ),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (mut ctx, _bufs) = test_context(transport, git(), config(), prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        let err = run(&ctx, list_args()).unwrap_err();
        let flag = err.downcast_ref::<FlagError>();
        assert!(flag.is_some(), "expected FlagError, got: {err}");
        assert!(
            err.to_string()
                .contains("issue tracker is not enabled for acme/widgets"),
            "msg: {err}"
        );
    }

    #[test]
    fn list_repo_not_found_404_is_distinct_error() {
        // #97: a plain 404 (typo'd/inaccessible repo) must NOT be reported as a
        // disabled tracker — it's a missing repository.
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list 404 missing repo",
            FakeTransport::rest(Method::Get, "/issues"),
            FakeTransport::json(
                404,
                r#"{"type":"error","error":{"message":"Repository acme/widgets no longer exists, or you may not have access."}}"#,
            ),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (mut ctx, _bufs) = test_context(transport, git(), config(), prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        let err = run(&ctx, list_args()).unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
        let msg = err.to_string();
        assert!(msg.contains("not found"), "should report not-found: {msg}");
        assert!(
            !msg.contains("tracker"),
            "must not mislabel a missing repo as a disabled tracker: {msg}"
        );
    }

    #[test]
    fn list_tracker_disabled_410_is_flag_error() {
        // #77: Bitbucket actually returns 410 Gone (not 404) for a disabled tracker.
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list 410",
            FakeTransport::rest(Method::Get, "/issues"),
            FakeTransport::json(410, r#"{"type":"error","error":{"message":"Gone"}}"#),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (mut ctx, _bufs) = test_context(transport, git(), config(), prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        let err = run(&ctx, list_args()).unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
        assert!(
            err.to_string()
                .contains("issue tracker is not enabled for acme/widgets"),
            "msg: {err}"
        );
    }

    #[test]
    fn list_json_emits_projected_fields() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list json",
            FakeTransport::rest(Method::Get, "/issues"),
            FakeTransport::json(200, TWO_ISSUES),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (mut ctx, bufs) = test_context(transport, git(), config(), prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        let a = ListArgs {
            json: crate::output::JsonFlags {
                json: vec!["id".into(), "title".into(), "state".into()],
                jq: None,
                template: None,
            },
            ..list_args()
        };
        run(&ctx, a).unwrap();

        let out = bufs.stdout_string();
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        let arr = v.as_array().expect("array");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["id"], 7);
        assert_eq!(arr[0]["title"], "Fix bug");
        assert_eq!(arr[0]["state"], "new");
        // Unrequested fields are projected away.
        assert!(arr[0].get("kind").is_none(), "out: {out}");
    }

    #[test]
    fn list_json_empty_is_empty_array() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list json empty",
            FakeTransport::rest(Method::Get, "/issues"),
            FakeTransport::json(200, r#"{"values": []}"#),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (mut ctx, bufs) = test_context(transport, git(), config(), prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        let a = ListArgs {
            json: crate::output::JsonFlags {
                json: vec!["id".into()],
                jq: None,
                template: None,
            },
            ..list_args()
        };
        run(&ctx, a).unwrap();

        let v: serde_json::Value = serde_json::from_str(&bufs.stdout_string()).expect("valid JSON");
        assert_eq!(v, serde_json::json!([]));
    }

    #[test]
    fn list_json_unknown_field_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list json bogus",
            FakeTransport::rest(Method::Get, "/issues"),
            FakeTransport::json(200, TWO_ISSUES),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (mut ctx, _bufs) = test_context(transport, git(), config(), prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        let a = ListArgs {
            json: crate::output::JsonFlags {
                json: vec!["bogus".into()],
                jq: None,
                template: None,
            },
            ..list_args()
        };
        let err = run(&ctx, a).unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some());
    }

    #[test]
    fn list_not_logged_in_is_auth_error() {
        let h = Arc::new(FakeTransport::new());
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let cfg: Arc<dyn ConfigProvider> = Arc::new(FileConfig::blank());
        let (mut ctx, _bufs) = test_context(transport, git(), cfg, prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        let err = run(&ctx, list_args()).unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some());
    }

    #[test]
    fn tsv_sanitizes_control_chars_in_title() {
        let issues: Vec<Issue> =
            vec![
                serde_json::from_str(r#"{"id":3,"title":"a\tb\nc","state":"new","kind":"bug"}"#)
                    .unwrap(),
            ];
        let out = render_tsv(&issues);
        assert_eq!(out, "3\ta b c\tnew\tbug\n");
        assert_eq!(out.matches('\n').count(), 1);
        assert_eq!(out.trim_end().matches('\t').count(), 3);
    }
}
