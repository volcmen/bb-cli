//! `bb pipeline list` — list recent CI pipelines for the current repository.

use bb_api::models::Pipeline;
use bb_api::BitbucketClient;
use bb_core::{AuthError, ColorScheme, Context};
use clap::Args;

use crate::auth;
use crate::render::{pad, sanitize};

/// JSON fields a pipeline can be projected to with `--json`.
const FIELDS: &[&str] = &["uuid", "build_number", "state", "target", "created_on"];

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Maximum number of pipelines to list
    #[arg(long, short = 'L', default_value_t = 20)]
    pub limit: usize,
    #[command(flatten)]
    pub json: crate::output::JsonFlags,
}

/// Run `bb pipeline list`.
///
/// # Errors
/// Returns [`AuthError`] (exit 4) if not authenticated for the repo's host, and
/// propagates [`ApiError`](bb_core::ApiError) from the listing call.
pub fn run(ctx: &Context, args: ListArgs) -> anyhow::Result<()> {
    let repo = ctx.base_repo()?;
    let host = repo.host().to_owned();

    let Some(header) = auth::header_for(ctx.config.as_ref(), &host) else {
        return Err(AuthError::new(host).into());
    };
    let client = BitbucketClient::new(ctx.transport.clone(), Some(header));

    // Bitbucket caps pagelen at 50; never request more than the user wants.
    // Note: the pipelines collection path carries a trailing slash.
    let pagelen = args.limit.clamp(1, 50);
    let path = format!(
        "/repositories/{}/{}/pipelines/?sort=-created_on&pagelen={pagelen}",
        repo.workspace(),
        repo.slug(),
    );

    let pipelines: Vec<Pipeline> = client.paginate(&path, Some(args.limit))?;

    if args.json.requested() {
        args.json.validate(FIELDS)?;
        args.json.emit(&ctx.io, serde_json::to_value(&pipelines)?)?;
        return Ok(());
    }

    if pipelines.is_empty() {
        ctx.io.println(&format!(
            "No pipelines found for {}/{}.",
            repo.workspace(),
            repo.slug()
        ));
        return Ok(());
    }

    if ctx.io.is_stdout_tty() {
        ctx.io
            .print(&render_table(&pipelines, ctx.io.color_scheme()));
    } else {
        ctx.io.print(&render_tsv(&pipelines));
    }
    Ok(())
}

/// The build-number cell for a pipeline (`#<n>` or `-` if absent).
fn build_cell(p: &Pipeline) -> String {
    match p.build_number {
        Some(n) => format!("#{n}"),
        None => "-".to_owned(),
    }
}

/// The target ref cell for a pipeline.
fn ref_cell(p: &Pipeline) -> String {
    sanitize(
        p.target
            .as_ref()
            .and_then(|t| t.ref_name.as_deref())
            .unwrap_or(""),
    )
}

/// Render a list of pipelines for a TTY: a header row plus aligned, colored columns.
fn render_table(pipelines: &[Pipeline], cs: ColorScheme) -> String {
    // Plain (uncolored) cell text, used for width computation.
    let rows: Vec<[String; 4]> = pipelines
        .iter()
        .map(|p| {
            [
                build_cell(p),
                sanitize(p.state_name()),
                sanitize(p.result_name()),
                ref_cell(p),
            ]
        })
        .collect();

    let headers = ["BUILD", "STATE", "RESULT", "REF"];
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
        // build (cyan), state (plain), result (colored), ref (plain)
        let build = cs.cyan(&row[0]);
        let result = color_result(cs, &row[2]);
        let cells = [build, row[1].clone(), result, row[3].clone()];
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

/// Render a list of pipelines for a pipe/script: one tab-separated line per
/// pipeline, no color and no header.
fn render_tsv(pipelines: &[Pipeline]) -> String {
    let mut out = String::new();
    for p in pipelines {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            build_cell(p),
            sanitize(p.state_name()),
            sanitize(p.result_name()),
            ref_cell(p),
        ));
    }
    out
}

/// Color a pipeline result: SUCCESSFUL green, FAILED/ERROR red, anything else gray.
fn color_result(cs: ColorScheme, result: &str) -> String {
    match result {
        "SUCCESSFUL" => cs.green(result),
        "FAILED" | "ERROR" => cs.red(result),
        other => cs.gray(other),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bb_api::testing::FakeTransport;
    use bb_config::FileConfig;
    use bb_core::{ConfigProvider, GitClient, Method, RepoId, Transport};
    use bb_git::{ShellGit, StubRunner};

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
            limit: 20,
            json: crate::output::JsonFlags::default(),
        }
    }

    const TWO_PIPELINES: &str = r#"{
        "values": [
            {
                "uuid": "{aaa}",
                "build_number": 12,
                "state": {"name": "COMPLETED", "result": {"name": "SUCCESSFUL"}},
                "target": {"ref_name": "main"},
                "created_on": "2026-06-15T10:00:00Z"
            },
            {
                "uuid": "{bbb}",
                "build_number": 11,
                "state": {"name": "COMPLETED", "result": {"name": "FAILED"}},
                "target": {"ref_name": "feature/x"},
                "created_on": "2026-06-14T10:00:00Z"
            }
        ]
    }"#;

    #[test]
    fn list_tsv_when_not_tty() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list",
            FakeTransport::rest(Method::Get, "/pipelines/"),
            FakeTransport::json(200, TWO_PIPELINES),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (mut ctx, bufs) = test_context(transport, git(), config(), prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        run(&ctx, list_args()).unwrap();

        let out = bufs.stdout_string();
        assert_eq!(
            out,
            "#12\tCOMPLETED\tSUCCESSFUL\tmain\n#11\tCOMPLETED\tFAILED\tfeature/x\n"
        );
        // No header row when piped.
        assert!(!out.contains("BUILD"));
    }

    #[test]
    fn list_table_when_tty() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list",
            FakeTransport::rest(Method::Get, "/pipelines/"),
            FakeTransport::json(200, TWO_PIPELINES),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (mut ctx, bufs) = test_context(transport, git(), config(), prompter, true);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        run(&ctx, list_args()).unwrap();

        let out = bufs.stdout_string();
        let first = out.lines().next().unwrap();
        assert!(first.contains("BUILD"));
        assert!(first.contains("STATE"));
        assert!(first.contains("RESULT"));
        assert!(first.contains("REF"));
        assert!(out.contains("#12"));
        assert!(out.contains("#11"));
        assert!(out.contains("SUCCESSFUL"));
        assert!(out.contains("FAILED"));
    }

    #[test]
    fn list_requests_sort_and_clamped_pagelen() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list sort",
            FakeTransport::rest(Method::Get, "/pipelines/"),
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
        assert!(url.contains("sort=-created_on"), "url: {url}");
        // Trailing slash on the collection.
        assert!(url.contains("/pipelines/?"), "url: {url}");
        // limit (5) < 50, so pagelen must be 5.
        assert!(url.contains("pagelen=5"), "url: {url}");
    }

    #[test]
    fn list_empty_prints_message() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list empty",
            FakeTransport::rest(Method::Get, "/pipelines/"),
            FakeTransport::json(200, r#"{"values": []}"#),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (mut ctx, bufs) = test_context(transport, git(), config(), prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        run(&ctx, list_args()).unwrap();
        assert!(bufs
            .stdout_string()
            .contains("No pipelines found for acme/widgets."));
    }

    #[test]
    fn list_json_emits_projected_fields() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list json",
            FakeTransport::rest(Method::Get, "/pipelines/"),
            FakeTransport::json(200, TWO_PIPELINES),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (mut ctx, bufs) = test_context(transport, git(), config(), prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        let a = ListArgs {
            json: crate::output::JsonFlags {
                json: vec!["uuid".into(), "build_number".into(), "state".into()],
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
        assert_eq!(arr[0]["uuid"], "{aaa}");
        assert_eq!(arr[0]["build_number"], 12);
        // The full nested state object is serialized through.
        assert_eq!(arr[0]["state"]["result"]["name"], "SUCCESSFUL");
        // Unrequested fields are projected away.
        assert!(arr[0].get("target").is_none(), "out: {out}");
    }

    #[test]
    fn list_json_empty_is_empty_array() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list json empty",
            FakeTransport::rest(Method::Get, "/pipelines/"),
            FakeTransport::json(200, r#"{"values": []}"#),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (mut ctx, bufs) = test_context(transport, git(), config(), prompter, false);
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        let a = ListArgs {
            json: crate::output::JsonFlags {
                json: vec!["uuid".into()],
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
            FakeTransport::rest(Method::Get, "/pipelines/"),
            FakeTransport::json(200, TWO_PIPELINES),
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
        assert!(err.downcast_ref::<bb_core::FlagError>().is_some());
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
    fn tsv_sanitizes_control_chars_in_ref() {
        let pipelines: Vec<Pipeline> = vec![serde_json::from_str(
            r#"{"build_number":3,"state":{"name":"COMPLETED","result":{"name":"SUCCESSFUL"}},"target":{"ref_name":"a\tb\nc"}}"#,
        )
        .unwrap()];
        let out = render_tsv(&pipelines);
        assert_eq!(out, "#3\tCOMPLETED\tSUCCESSFUL\ta b c\n");
        assert_eq!(out.matches('\n').count(), 1);
        assert_eq!(out.trim_end().matches('\t').count(), 3);
    }

    #[test]
    fn render_table_colors_result_when_enabled() {
        let (mut io, _) = bb_core::IoStreams::test();
        io.set_stdout_tty(true);
        let cs = io.color_scheme();
        let pipelines: Vec<Pipeline> = serde_json::from_str(TWO_PIPELINES)
            .map(|p: serde_json::Value| {
                p["values"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| serde_json::from_value(v.clone()).unwrap())
                    .collect()
            })
            .unwrap();
        let out = render_table(&pipelines, cs);
        // SUCCESSFUL is green (32), FAILED is red (31).
        assert!(out.contains("\x1b[32mSUCCESSFUL"), "out: {out}");
        assert!(out.contains("\x1b[31mFAILED"), "out: {out}");
    }
}
