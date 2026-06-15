//! `bb repo list` — list repositories in a workspace.

use bb_api::models::Repository;
use bb_api::BitbucketClient;
use bb_core::{AuthError, ColorScheme, Context, FlagError};
use clap::Args;

use crate::auth;

/// JSON fields a repository can be projected to with `--json`.
const FIELDS: &[&str] = &[
    "slug",
    "name",
    "full_name",
    "is_private",
    "description",
    "mainbranch",
    "links",
];

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Workspace to list repositories for (defaults to the current repo's workspace)
    #[arg(value_name = "WORKSPACE")]
    pub workspace: Option<String>,
    /// Maximum number of repositories to list
    #[arg(long, short = 'L', default_value_t = 30)]
    pub limit: usize,
    #[command(flatten)]
    pub json: crate::output::JsonFlags,
}

/// Run `bb repo list`.
///
/// # Errors
/// Returns [`AuthError`] (exit 4) if not authenticated for the target host,
/// [`FlagError`] (exit 1) if no workspace is given and none can be inferred, and
/// propagates [`ApiError`](bb_core::ApiError) from the listing call.
pub fn run(ctx: &Context, args: ListArgs) -> anyhow::Result<()> {
    let workspace = match args.workspace {
        Some(w) => w,
        // No arg: fall back to the current repo's workspace, but only error on
        // the *absence* of a workspace — git failures shouldn't mask the hint.
        None => ctx
            .base_repo()
            .map(|r| r.workspace().to_owned())
            .map_err(|_| FlagError::new("specify a WORKSPACE to list repositories for"))?,
    };

    let host = ctx.host();
    let header = auth::header_for(ctx.config.as_ref(), &host);
    if header.is_none() {
        return Err(AuthError::new(host).into());
    }
    let client = BitbucketClient::new(ctx.transport.clone(), header);

    let pagelen = args.limit.clamp(1, 100);
    let path = format!("/repositories/{workspace}?pagelen={pagelen}&sort=-updated_on");
    let repos: Vec<Repository> = client.paginate(&path, Some(args.limit))?;

    if args.json.requested() {
        args.json.validate(FIELDS)?;
        args.json.emit(&ctx.io, serde_json::to_value(&repos)?)?;
        return Ok(());
    }

    if repos.is_empty() {
        ctx.io
            .println(&format!("No repositories found in {workspace}."));
        return Ok(());
    }

    if ctx.io.is_stdout_tty() {
        ctx.io.print(&render_table(&repos, ctx.io.color_scheme()));
    } else {
        ctx.io.print(&render_tsv(&repos));
    }
    Ok(())
}

/// The display cells for a repository: name, visibility, default branch.
fn cells(r: &Repository) -> [String; 3] {
    let name = r
        .full_name
        .clone()
        .or_else(|| r.slug.clone())
        .unwrap_or_default();
    let visibility = match r.is_private {
        Some(true) => "private",
        _ => "public",
    };
    let branch = r.mainbranch.as_ref().map_or("", |b| b.name.as_str());
    [sanitize(&name), visibility.to_owned(), sanitize(branch)]
}

/// Tab-separated, no header, no color (for pipes/scripts).
fn render_tsv(repos: &[Repository]) -> String {
    let mut out = String::new();
    for r in repos {
        let [name, vis, branch] = cells(r);
        out.push_str(&format!("{name}\t{vis}\t{branch}\n"));
    }
    out
}

/// A TTY table: a header row plus aligned columns (name colored).
fn render_table(repos: &[Repository], cs: ColorScheme) -> String {
    let rows: Vec<[String; 3]> = repos.iter().map(cells).collect();

    let headers = ["NAME", "VISIBILITY", "BRANCH"];
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
        let cells = [cs.cyan(&row[0]), row[1].clone(), row[2].clone()];
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

/// Collapse control characters that would corrupt terminal/TSV output.
fn sanitize(s: &str) -> String {
    s.replace(['\t', '\r', '\n'], " ")
}

/// Pad `s` (whose visible width is `plain_len`) on the right to `target`.
fn pad(s: &str, plain_len: usize, target: usize) -> String {
    let mut out = s.to_owned();
    if plain_len < target {
        out.push_str(&" ".repeat(target - plain_len));
    }
    out
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

    /// Git stub answering `remote -v` (so `base_repo` can default).
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

    /// Git stub that errors on `remote -v` (no Bitbucket remote present).
    fn git_no_remote() -> Arc<dyn GitClient> {
        let s = Arc::new(StubRunner::new());
        s.register("remote -v", 0, "");
        Arc::new(ShellGit::new(s))
    }

    /// Git stub that must never be called (an explicit workspace is passed).
    fn no_git() -> Arc<dyn GitClient> {
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

    fn list_args(workspace: Option<&str>) -> ListArgs {
        ListArgs {
            workspace: workspace.map(ToOwned::to_owned),
            limit: 30,
            json: crate::output::JsonFlags::default(),
        }
    }

    const TWO_REPOS: &str = r#"{
        "values": [
            {"slug":"widgets","full_name":"acme/widgets","is_private":true,
             "mainbranch":{"name":"main"}},
            {"slug":"gadgets","full_name":"acme/gadgets","is_private":false,
             "mainbranch":{"name":"trunk"}}
        ]
    }"#;

    #[test]
    fn list_tsv_when_not_tty() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list",
            FakeTransport::rest(Method::Get, "/repositories/acme"),
            FakeTransport::json(200, TWO_REPOS),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, bufs) = test_context(transport, no_git(), config(), prompter, false);

        run(&ctx, list_args(Some("acme"))).unwrap();

        let out = bufs.stdout_string();
        assert_eq!(
            out,
            "acme/widgets\tprivate\tmain\nacme/gadgets\tpublic\ttrunk\n"
        );
        assert!(!out.contains("NAME"), "TSV must have no header");
    }

    #[test]
    fn list_table_when_tty() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list",
            FakeTransport::rest(Method::Get, "/repositories/acme"),
            FakeTransport::json(200, TWO_REPOS),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, bufs) = test_context(transport, no_git(), config(), prompter, true);

        run(&ctx, list_args(Some("acme"))).unwrap();

        let out = bufs.stdout_string();
        let first = out.lines().next().unwrap();
        assert!(first.contains("NAME"), "out: {out}");
        assert!(first.contains("VISIBILITY"), "out: {out}");
        assert!(out.contains("acme/widgets"));
        assert!(out.contains("acme/gadgets"));
        assert!(out.contains("private"));
    }

    #[test]
    fn list_defaults_to_base_repo_workspace() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list",
            FakeTransport::rest(Method::Get, "/repositories/acme"),
            FakeTransport::json(200, TWO_REPOS),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, bufs) = test_context(transport, git(), config(), prompter, false);

        run(&ctx, list_args(None)).unwrap();
        assert!(bufs.stdout_string().contains("acme/widgets"));
    }

    #[test]
    fn list_no_workspace_and_no_repo_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, _bufs) = test_context(transport, git_no_remote(), config(), prompter, false);

        let err = run(&ctx, list_args(None)).unwrap_err();
        let flag = err.downcast_ref::<FlagError>();
        assert!(flag.is_some(), "expected FlagError, got: {err}");
        assert!(flag.unwrap().0.contains("specify a WORKSPACE"));
    }

    #[test]
    fn list_empty_prints_message() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list empty",
            FakeTransport::rest(Method::Get, "/repositories/acme"),
            FakeTransport::json(200, r#"{"values": []}"#),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, bufs) = test_context(transport, no_git(), config(), prompter, false);

        run(&ctx, list_args(Some("acme"))).unwrap();
        assert!(bufs
            .stdout_string()
            .contains("No repositories found in acme."));
    }

    #[test]
    fn list_pagelen_clamped_and_sorted() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list big limit",
            FakeTransport::rest(Method::Get, "/repositories/acme"),
            FakeTransport::json(200, r#"{"values": []}"#),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, _bufs) = test_context(transport, no_git(), config(), prompter, false);

        let a = ListArgs {
            limit: 250,
            ..list_args(Some("acme"))
        };
        run(&ctx, a).unwrap();

        let reqs = h.requests.lock().unwrap();
        let url = &reqs[0].url;
        // 250 clamps to the API max of 100.
        assert!(url.contains("pagelen=100"), "url: {url}");
        assert!(url.contains("sort=-updated_on"), "url: {url}");
    }

    #[test]
    fn list_json_emits_projected_fields() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list json",
            FakeTransport::rest(Method::Get, "/repositories/acme"),
            FakeTransport::json(200, TWO_REPOS),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, bufs) = test_context(transport, no_git(), config(), prompter, false);

        let a = ListArgs {
            json: crate::output::JsonFlags {
                json: vec!["slug".into(), "full_name".into(), "is_private".into()],
                jq: None,
                template: None,
            },
            ..list_args(Some("acme"))
        };
        run(&ctx, a).unwrap();

        let out = bufs.stdout_string();
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        let arr = v.as_array().expect("array");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["slug"], "widgets");
        assert_eq!(arr[0]["full_name"], "acme/widgets");
        assert_eq!(arr[0]["is_private"], true);
        // Unrequested fields are projected away.
        assert!(arr[0].get("mainbranch").is_none(), "out: {out}");
    }

    #[test]
    fn list_json_empty_is_empty_array() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list json empty",
            FakeTransport::rest(Method::Get, "/repositories/acme"),
            FakeTransport::json(200, r#"{"values": []}"#),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, bufs) = test_context(transport, no_git(), config(), prompter, false);

        let a = ListArgs {
            json: crate::output::JsonFlags {
                json: vec!["slug".into()],
                jq: None,
                template: None,
            },
            ..list_args(Some("acme"))
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
            FakeTransport::rest(Method::Get, "/repositories/acme"),
            FakeTransport::json(200, TWO_REPOS),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, _bufs) = test_context(transport, no_git(), config(), prompter, false);

        let a = ListArgs {
            json: crate::output::JsonFlags {
                json: vec!["bogus".into()],
                jq: None,
                template: None,
            },
            ..list_args(Some("acme"))
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
        let (ctx, _bufs) = test_context(transport, no_git(), cfg, prompter, false);

        let err = run(&ctx, list_args(Some("acme"))).unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some());
    }
}
