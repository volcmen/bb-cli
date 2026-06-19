//! `bb issue edit` / `close` / `reopen` — update an issue or its state.

use crate::api::{BitbucketClient, Issue};
use crate::core::{AuthError, Context, FlagError};
use clap::Args;

#[derive(Args, Debug)]
pub struct EditArgs {
    /// Issue id
    #[arg(value_name = "ID")]
    pub id: String,
    /// New title
    #[arg(long, short = 't')]
    pub title: Option<String>,
    /// New body/content
    #[arg(long, short = 'b')]
    pub body: Option<String>,
    /// Read the new body from a file (use "-" for stdin)
    #[arg(long = "body-file", short = 'F', value_name = "FILE")]
    pub body_file: Option<String>,
    /// New kind
    #[arg(long, value_parser = ["bug", "enhancement", "proposal", "task"])]
    pub kind: Option<String>,
    /// New priority
    #[arg(long, value_parser = ["trivial", "minor", "major", "critical", "blocker"])]
    pub priority: Option<String>,
    /// New state
    #[arg(long, value_parser = ["new", "open", "resolved", "on hold", "invalid", "duplicate", "wontfix", "closed"])]
    pub state: Option<String>,
}

#[derive(Args, Debug)]
pub struct StateArgs {
    /// Issue id
    #[arg(value_name = "ID")]
    pub id: String,
}

#[derive(serde::Serialize)]
struct Content<'a> {
    raw: &'a str,
}

#[derive(serde::Serialize)]
struct UpdateIssueBody<'a> {
    title: &'a str,
    content: Content<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    priority: Option<&'a str>,
    state: &'a str,
}

/// Field overrides applied on top of the current issue.
#[derive(Default)]
struct Overrides {
    title: Option<String>,
    body: Option<String>,
    kind: Option<String>,
    priority: Option<String>,
    state: Option<String>,
}

/// Run `bb issue edit`.
///
/// # Errors
/// [`AuthError`] (4) unauthenticated; [`FlagError`] (1) when no field is given /
/// the tracker is disabled / the repository is missing/inaccessible / the issue
/// is missing; propagates API/IO errors.
pub fn run(ctx: &Context, args: EditArgs) -> anyhow::Result<()> {
    if args.title.is_none()
        && args.body.is_none()
        && args.body_file.is_none()
        && args.kind.is_none()
        && args.priority.is_none()
        && args.state.is_none()
    {
        return Err(FlagError::new(
            "nothing to update; pass --title, --body/--body-file, --kind, --priority, or --state",
        )
        .into());
    }
    let body = resolve_body(ctx, &args)?;
    let overrides = Overrides {
        title: args.title,
        body,
        kind: args.kind,
        priority: args.priority,
        state: args.state,
    };
    update(ctx, &args.id, overrides, "Updated")
}

/// Run `bb issue close` (state → resolved).
///
/// # Errors
/// As [`run`].
pub fn run_close(ctx: &Context, args: StateArgs) -> anyhow::Result<()> {
    update(
        ctx,
        &args.id,
        Overrides {
            state: Some("resolved".to_owned()),
            ..Overrides::default()
        },
        "Closed",
    )
}

/// Run `bb issue reopen` (state → open).
///
/// # Errors
/// As [`run`].
pub fn run_reopen(ctx: &Context, args: StateArgs) -> anyhow::Result<()> {
    update(
        ctx,
        &args.id,
        Overrides {
            state: Some("open".to_owned()),
            ..Overrides::default()
        },
        "Reopened",
    )
}

/// Fetch the issue, apply `overrides`, and PUT the merged result.
fn update(ctx: &Context, id: &str, overrides: Overrides, verb: &str) -> anyhow::Result<()> {
    let repo = ctx.base_repo()?;
    let host = repo.host().to_owned();
    let Some(header) = crate::auth::header_for(ctx.config.as_ref(), &host) else {
        return Err(AuthError::new(host).into());
    };
    let client = BitbucketClient::new(ctx.transport.clone(), Some(header));

    let path = format!(
        "/repositories/{}/{}/issues/{id}",
        repo.workspace(),
        repo.slug()
    );
    let current: Issue = match client.get(&path) {
        Ok(i) => i,
        // 410 = tracker disabled. A 404 is ambiguous: disabled tracker, missing
        // repo, or missing issue — decide by the body message.
        Err(e) if e.is_gone() => return Err(super::tracker_disabled(&repo).into()),
        Err(e) if e.is_not_found() => return Err(super::issue_level_404(&repo, id, &e).into()),
        Err(e) => return Err(e.into()),
    };

    let title = overrides.title.or(current.title).unwrap_or_default();
    let body = overrides
        .body
        .or_else(|| current.content.and_then(|c| c.raw))
        .unwrap_or_default();
    let kind = overrides.kind.or(current.kind);
    let priority = overrides.priority.or(current.priority);
    let state = overrides
        .state
        .or(current.state)
        .unwrap_or_else(|| "new".to_owned());

    let payload = UpdateIssueBody {
        title: &title,
        content: Content { raw: &body },
        kind: kind.as_deref(),
        priority: priority.as_deref(),
        state: &state,
    };
    let _updated: Issue = client.put(&path, &payload)?;
    ctx.io.println(&format!("✓ {verb} issue #{id}"));
    Ok(())
}

/// Resolve the body override from `--body`, then `--body-file` (`-` => stdin).
fn resolve_body(ctx: &Context, args: &EditArgs) -> anyhow::Result<Option<String>> {
    if let Some(b) = &args.body {
        return Ok(Some(b.clone()));
    }
    if let Some(file) = &args.body_file {
        if file == "-" {
            return Ok(Some(ctx.io.read_stdin_to_string()?));
        }
        return Ok(Some(std::fs::read_to_string(file)?));
    }
    Ok(None)
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

    fn ctx_with(http: Arc<FakeTransport>, config: Arc<dyn ConfigProvider>) -> Context {
        let transport: Arc<dyn Transport> = http;
        let (mut ctx, _bufs) = test_context(
            transport,
            git(),
            config,
            Arc::new(ScriptedPrompter::new()),
            false,
        );
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));
        ctx
    }

    const ISSUE: &str = r#"{"id":5,"title":"Old","state":"new","kind":"bug",
        "priority":"major","content":{"raw":"old body"}}"#;

    fn stub_get(h: &Arc<FakeTransport>) {
        h.stub(
            "get issue",
            FakeTransport::rest(Method::Get, "/issues/5"),
            FakeTransport::json(200, ISSUE),
        );
    }
    fn stub_put(h: &Arc<FakeTransport>) {
        h.stub(
            "put issue",
            FakeTransport::rest(Method::Put, "/issues/5"),
            FakeTransport::json(200, ISSUE),
        );
    }
    fn put_body(h: &FakeTransport) -> serde_json::Value {
        let reqs = h.requests.lock().unwrap();
        let put = reqs.iter().find(|r| r.method == Method::Put).unwrap();
        serde_json::from_slice(put.body.as_deref().unwrap()).unwrap()
    }
    fn edit_args(id: &str) -> EditArgs {
        EditArgs {
            id: id.to_owned(),
            title: None,
            body: None,
            body_file: None,
            kind: None,
            priority: None,
            state: None,
        }
    }

    #[test]
    fn edit_updates_title_and_preserves_rest() {
        let h = Arc::new(FakeTransport::new());
        stub_get(&h);
        stub_put(&h);
        let ctx = ctx_with(h.clone(), authed_config());

        run(
            &ctx,
            EditArgs {
                title: Some("New".to_owned()),
                ..edit_args("5")
            },
        )
        .unwrap();

        let body = put_body(&h);
        assert_eq!(body["title"], "New");
        assert_eq!(body["content"]["raw"], "old body");
        assert_eq!(body["state"], "new");
        assert_eq!(body["kind"], "bug");
    }

    #[test]
    fn edit_state_change() {
        let h = Arc::new(FakeTransport::new());
        stub_get(&h);
        stub_put(&h);
        let ctx = ctx_with(h.clone(), authed_config());
        run(
            &ctx,
            EditArgs {
                state: Some("resolved".to_owned()),
                ..edit_args("5")
            },
        )
        .unwrap();
        assert_eq!(put_body(&h)["state"], "resolved");
    }

    #[test]
    fn edit_no_fields_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        let ctx = ctx_with(h.clone(), authed_config());
        let err = run(&ctx, edit_args("5")).unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
        assert_eq!(h.request_count(), 0);
    }

    #[test]
    fn close_sets_resolved() {
        let h = Arc::new(FakeTransport::new());
        stub_get(&h);
        stub_put(&h);
        let ctx = ctx_with(h.clone(), authed_config());
        run_close(&ctx, StateArgs { id: "5".to_owned() }).unwrap();
        assert_eq!(put_body(&h)["state"], "resolved");
    }

    #[test]
    fn reopen_sets_open() {
        let h = Arc::new(FakeTransport::new());
        stub_get(&h);
        stub_put(&h);
        let ctx = ctx_with(h.clone(), authed_config());
        run_reopen(&ctx, StateArgs { id: "5".to_owned() }).unwrap();
        assert_eq!(put_body(&h)["state"], "open");
    }

    #[test]
    fn edit_tracker_disabled_410_reports_tracker() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get 410",
            FakeTransport::rest(Method::Get, "/issues/5"),
            FakeTransport::json(410, r#"{"type":"error","error":{"message":"Gone"}}"#),
        );
        let ctx = ctx_with(h.clone(), authed_config());
        let err = run(
            &ctx,
            EditArgs {
                title: Some("x".to_owned()),
                ..edit_args("5")
            },
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("issue tracker is not enabled"),
            "got: {err}"
        );
    }

    #[test]
    fn edit_tracker_disabled_404_body_reports_tracker() {
        // A 404 whose body says the repo has no issue tracker => tracker-disabled.
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get 404 no tracker",
            FakeTransport::rest(Method::Get, "/issues/5"),
            FakeTransport::json(
                404,
                r#"{"type":"error","error":{"message":"Repository has no issue tracker."}}"#,
            ),
        );
        let ctx = ctx_with(h.clone(), authed_config());
        let err = run(
            &ctx,
            EditArgs {
                title: Some("x".to_owned()),
                ..edit_args("5")
            },
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("issue tracker is not enabled"),
            "got: {err}"
        );
    }

    #[test]
    fn edit_repo_not_found_404_reports_repo() {
        // #97: a 404 pointing at the repository must report a missing repo.
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get 404 missing repo",
            FakeTransport::rest(Method::Get, "/issues/5"),
            FakeTransport::json(
                404,
                r#"{"type":"error","error":{"message":"Repository acme/widgets no longer exists, or you may not have access."}}"#,
            ),
        );
        let ctx = ctx_with(h.clone(), authed_config());
        let err = run(
            &ctx,
            EditArgs {
                title: Some("x".to_owned()),
                ..edit_args("5")
            },
        )
        .unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
        let msg = err.to_string();
        assert!(msg.contains("not found"), "should report not-found: {msg}");
        assert!(!msg.contains("tracker"), "must not say tracker: {msg}");
    }

    #[test]
    fn edit_issue_not_found_404_reports_issue() {
        // A 404 naming the issue (not repo/tracker) => the issue is missing.
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get 404 missing issue",
            FakeTransport::rest(Method::Get, "/issues/5"),
            FakeTransport::json(
                404,
                r#"{"type":"error","error":{"message":"No such issue."}}"#,
            ),
        );
        let ctx = ctx_with(h.clone(), authed_config());
        let err = run(
            &ctx,
            EditArgs {
                title: Some("x".to_owned()),
                ..edit_args("5")
            },
        )
        .unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
        assert!(err.to_string().contains("issue #5 not found"), "got: {err}");
    }

    #[test]
    fn edit_not_authed_is_auth_error() {
        let h = Arc::new(FakeTransport::new());
        let ctx = ctx_with(h.clone(), Arc::new(FileConfig::blank()));
        let err = run(
            &ctx,
            EditArgs {
                title: Some("x".to_owned()),
                ..edit_args("5")
            },
        )
        .unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some(), "got: {err}");
    }
}
