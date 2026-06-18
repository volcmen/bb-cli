//! `bb pr checks` — show build/CI statuses for a PR's head commit.
//!
//! The Bitbucket analog of `gh pr checks`: resolve the PR (by id or current
//! branch), find its head commit, and list the build statuses attached to that
//! commit. As with `gh`, if any check has failed the process exits non-zero
//! (after printing the table), so it can gate CI.

use crate::api::models::CommitStatus;
use crate::api::BitbucketClient;
use crate::core::{AuthError, ColorScheme, Context, SilentError};
use clap::Args;

use crate::auth;
use crate::render::{pad, sanitize};

/// JSON fields a check (commit status) can be projected to with `--json`.
const FIELDS: &[&str] = &["key", "name", "state", "url"];

#[derive(Args, Debug)]
pub struct ChecksArgs {
    /// Pull request id (defaults to the PR for the current branch)
    #[arg(value_name = "ID")]
    pub id: Option<String>,
    #[command(flatten)]
    pub json: crate::output::JsonFlags,
}

/// Run `bb pr checks`.
///
/// # Errors
/// Returns [`AuthError`] (exit 4) if not authenticated for the repo's host,
/// propagates errors from PR resolution / the statuses call, and returns
/// [`SilentError`] (exit 1) — *after* printing the table — when any check has
/// failed, so the command can gate CI.
pub fn run(ctx: &Context, args: ChecksArgs) -> anyhow::Result<()> {
    let repo = ctx.base_repo()?;
    let host = repo.host().to_owned();

    let Some(header) = auth::header_for(ctx.config.as_ref(), &host) else {
        return Err(AuthError::new(host).into());
    };
    let client = BitbucketClient::new(ctx.transport.clone(), Some(header));

    let pr = super::finder::resolve(ctx, &client, &repo, args.id.as_deref())?;
    let sha = pr
        .source
        .commit_hash()
        .ok_or_else(|| anyhow::anyhow!("could not determine the head commit for PR #{}", pr.id))?;

    let statuses: Vec<CommitStatus> = super::query::checks(&client, &repo, sha)?;

    if args.json.requested() {
        args.json.validate(FIELDS)?;
        args.json.emit(&ctx.io, serde_json::to_value(&statuses)?)?;
        return Ok(());
    }

    if statuses.is_empty() {
        ctx.io
            .println(&format!("No checks reported for PR #{}.", pr.id));
        return Ok(());
    }

    if ctx.io.is_stdout_tty() {
        ctx.io
            .print(&render_table(&statuses, ctx.io.color_scheme()));
    } else {
        ctx.io.print(&render_tsv(&statuses));
    }

    // CI-friendly: surface a non-zero exit if any check failed, but only after
    // the table has been shown.
    if statuses.iter().any(CommitStatus::is_failed) {
        return Err(SilentError.into());
    }
    Ok(())
}

/// Render checks for a TTY: a header row plus aligned columns with the state
/// colored by outcome.
fn render_table(statuses: &[CommitStatus], cs: ColorScheme) -> String {
    let rows: Vec<[String; 3]> = statuses
        .iter()
        .map(|s| {
            [
                sanitize(state_of(s)),
                sanitize(name_of(s)),
                sanitize(url_of(s)),
            ]
        })
        .collect();

    let headers = ["STATE", "NAME", "URL"];
    let mut widths = headers.map(str::len);
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }

    let mut out = String::new();
    for (i, h) in headers.iter().enumerate() {
        out.push_str(&pad(&cs.bold(h), h.chars().count(), widths[i]));
        if i + 1 < headers.len() {
            out.push_str("  ");
        }
    }
    out.push('\n');

    for row in &rows {
        let state = color_state(cs, &row[0]);
        let cells = [state, row[1].clone(), row[2].clone()];
        let plain_lens = [
            row[0].chars().count(),
            row[1].chars().count(),
            row[2].chars().count(),
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

/// Render checks for a pipe/script: one tab-separated `state\tname\turl` line
/// per check, no color and no header.
fn render_tsv(statuses: &[CommitStatus]) -> String {
    let mut out = String::new();
    for s in statuses {
        out.push_str(&format!(
            "{}\t{}\t{}\n",
            sanitize(state_of(s)),
            sanitize(name_of(s)),
            sanitize(url_of(s)),
        ));
    }
    out
}

fn state_of(s: &CommitStatus) -> &str {
    s.state.as_deref().unwrap_or("")
}

/// The display name: `name` if present, otherwise the `key`.
fn name_of(s: &CommitStatus) -> &str {
    s.name
        .as_deref()
        .filter(|n| !n.is_empty())
        .or(s.key.as_deref())
        .unwrap_or("")
}

fn url_of(s: &CommitStatus) -> &str {
    s.url.as_deref().unwrap_or("")
}

fn color_state(cs: ColorScheme, state: &str) -> String {
    match state {
        "SUCCESSFUL" => cs.green(state),
        "FAILED" => cs.red(state),
        other => cs.yellow(other),
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

    fn ctx_with(
        http: Arc<FakeTransport>,
        config: Arc<dyn ConfigProvider>,
        tty: bool,
    ) -> (Context, crate::core::TestBuffers) {
        let git: Arc<dyn GitClient> = Arc::new(ShellGit::new(Arc::new(StubRunner::new())));
        let transport: Arc<dyn Transport> = http;
        let (mut ctx, bufs) = test_context(
            transport,
            git,
            config,
            Arc::new(ScriptedPrompter::new()),
            tty,
        );
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));
        (ctx, bufs)
    }

    /// Stub the `GET /pullrequests/{id}` that `finder::resolve(Some(id))` makes,
    /// returning a PR whose `source.commit.hash` is `sha`.
    fn stub_pr(h: &FakeTransport, id: u64, sha: &str) {
        h.stub(
            "GET pr",
            FakeTransport::rest(Method::Get, &format!("/pullrequests/{id}")),
            FakeTransport::json(
                200,
                &format!(
                    r#"{{"id":{id},"title":"T","state":"OPEN",
                        "source":{{"branch":{{"name":"feat"}},"commit":{{"hash":"{sha}"}}}},
                        "destination":{{"branch":{{"name":"main"}}}}}}"#
                ),
            ),
        );
    }

    fn stub_statuses(h: &FakeTransport, sha: &str, body: &str) {
        h.stub(
            "GET statuses",
            FakeTransport::rest(Method::Get, &format!("/commit/{sha}/statuses")),
            FakeTransport::json(200, body),
        );
    }

    fn checks_args(id: &str) -> ChecksArgs {
        ChecksArgs {
            id: Some(id.to_owned()),
            json: crate::output::JsonFlags::default(),
        }
    }

    const TWO_PASSING: &str = r#"{
        "values": [
            {"key": "BUILD", "name": "Build", "state": "SUCCESSFUL",
             "url": "https://ci/build/1"},
            {"key": "LINT", "name": "Lint", "state": "SUCCESSFUL",
             "url": "https://ci/lint/1"}
        ]
    }"#;

    #[test]
    fn checks_renders_table_when_tty() {
        let h = Arc::new(FakeTransport::new());
        stub_pr(&h, 42, "abc123");
        stub_statuses(&h, "abc123", TWO_PASSING);
        let (ctx, bufs) = ctx_with(h.clone(), authed_config(), true);

        run(&ctx, checks_args("42")).unwrap();

        let out = bufs.stdout_string();
        let first = out.lines().next().unwrap();
        assert!(first.contains("STATE"), "out: {out}");
        assert!(first.contains("NAME"));
        assert!(first.contains("URL"));
        assert!(out.contains("Build"));
        assert!(out.contains("Lint"));
        assert!(out.contains("https://ci/build/1"));
    }

    #[test]
    fn checks_tsv_when_not_tty() {
        let h = Arc::new(FakeTransport::new());
        stub_pr(&h, 42, "abc123");
        stub_statuses(&h, "abc123", TWO_PASSING);
        let (ctx, bufs) = ctx_with(h.clone(), authed_config(), false);

        run(&ctx, checks_args("42")).unwrap();

        let out = bufs.stdout_string();
        assert_eq!(
            out,
            "SUCCESSFUL\tBuild\thttps://ci/build/1\n\
             SUCCESSFUL\tLint\thttps://ci/lint/1\n"
        );
        assert!(!out.contains("STATE"));
    }

    #[test]
    fn checks_all_passing_returns_ok() {
        let h = Arc::new(FakeTransport::new());
        stub_pr(&h, 42, "abc123");
        stub_statuses(&h, "abc123", TWO_PASSING);
        let (ctx, _bufs) = ctx_with(h.clone(), authed_config(), false);

        assert!(run(&ctx, checks_args("42")).is_ok());
    }

    #[test]
    fn checks_any_failed_returns_silent_error_but_still_prints_table() {
        let h = Arc::new(FakeTransport::new());
        stub_pr(&h, 42, "abc123");
        stub_statuses(
            &h,
            "abc123",
            r#"{"values":[
                {"key":"BUILD","name":"Build","state":"SUCCESSFUL","url":"https://ci/b"},
                {"key":"TEST","name":"Test","state":"FAILED","url":"https://ci/t"}
            ]}"#,
        );
        let (ctx, bufs) = ctx_with(h.clone(), authed_config(), false);

        let err = run(&ctx, checks_args("42")).unwrap_err();
        assert!(
            err.downcast_ref::<SilentError>().is_some(),
            "expected SilentError, got: {err}"
        );

        // The table/TSV must still have been printed before the non-zero exit.
        let out = bufs.stdout_string();
        assert!(out.contains("FAILED"), "out: {out}");
        assert!(out.contains("Test"));
        assert!(out.contains("Build"));
    }

    #[test]
    fn checks_empty_prints_message_and_returns_ok() {
        let h = Arc::new(FakeTransport::new());
        stub_pr(&h, 42, "abc123");
        stub_statuses(&h, "abc123", r#"{"values":[]}"#);
        let (ctx, bufs) = ctx_with(h.clone(), authed_config(), false);

        run(&ctx, checks_args("42")).unwrap();
        assert!(bufs
            .stdout_string()
            .contains("No checks reported for PR #42."));
    }

    #[test]
    fn checks_json_emits_projected_fields() {
        let h = Arc::new(FakeTransport::new());
        stub_pr(&h, 42, "abc123");
        stub_statuses(&h, "abc123", TWO_PASSING);
        let (ctx, bufs) = ctx_with(h.clone(), authed_config(), false);

        let a = ChecksArgs {
            id: Some("42".to_owned()),
            json: crate::output::JsonFlags {
                json: vec!["key".into(), "state".into()],
                jq: None,
                template: None,
            },
        };
        run(&ctx, a).unwrap();

        let v: serde_json::Value = serde_json::from_str(&bufs.stdout_string()).expect("valid JSON");
        let arr = v.as_array().expect("array");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["key"], "BUILD");
        assert_eq!(arr[0]["state"], "SUCCESSFUL");
        // Unrequested fields are projected away.
        assert!(arr[0].get("name").is_none());
        assert!(arr[0].get("url").is_none());
    }

    #[test]
    fn checks_json_empty_is_empty_array() {
        let h = Arc::new(FakeTransport::new());
        stub_pr(&h, 42, "abc123");
        stub_statuses(&h, "abc123", r#"{"values":[]}"#);
        let (ctx, bufs) = ctx_with(h.clone(), authed_config(), false);

        let a = ChecksArgs {
            id: Some("42".to_owned()),
            json: crate::output::JsonFlags {
                json: vec!["key".into()],
                jq: None,
                template: None,
            },
        };
        run(&ctx, a).unwrap();

        let v: serde_json::Value = serde_json::from_str(&bufs.stdout_string()).expect("valid JSON");
        assert_eq!(v, serde_json::json!([]));
    }

    #[test]
    fn checks_json_does_not_gate_on_failure() {
        // `--json` short-circuits before the failure check, so a FAILED status
        // still returns Ok (machine consumers inspect the JSON themselves).
        let h = Arc::new(FakeTransport::new());
        stub_pr(&h, 42, "abc123");
        stub_statuses(
            &h,
            "abc123",
            r#"{"values":[{"key":"TEST","name":"Test","state":"FAILED","url":"u"}]}"#,
        );
        let (ctx, _bufs) = ctx_with(h.clone(), authed_config(), false);

        let a = ChecksArgs {
            id: Some("42".to_owned()),
            json: crate::output::JsonFlags {
                json: vec!["state".into()],
                jq: None,
                template: None,
            },
        };
        assert!(run(&ctx, a).is_ok());
    }

    #[test]
    fn checks_not_authed_returns_auth_error_before_network() {
        let h = Arc::new(FakeTransport::new());
        // No stubs registered: if any request were made, FakeTransport would
        // panic. The auth gate must fire first.
        let (ctx, _bufs) = ctx_with(h.clone(), Arc::new(FileConfig::blank()), false);

        let err = run(&ctx, checks_args("42")).unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some());
    }

    #[test]
    fn checks_missing_head_commit_is_error() {
        let h = Arc::new(FakeTransport::new());
        // PR with no source.commit.hash.
        h.stub(
            "GET pr no commit",
            FakeTransport::rest(Method::Get, "/pullrequests/42"),
            FakeTransport::json(
                200,
                r#"{"id":42,"title":"T","state":"OPEN",
                    "source":{"branch":{"name":"feat"}},
                    "destination":{"branch":{"name":"main"}}}"#,
            ),
        );
        let (ctx, _bufs) = ctx_with(h.clone(), authed_config(), false);

        let err = run(&ctx, checks_args("42")).unwrap_err();
        assert!(
            err.to_string().contains("head commit"),
            "unexpected error: {err}"
        );
    }
}
