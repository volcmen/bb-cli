//! `bb pr list` — list pull requests for the current repository.

use bb_api::models::PullRequest;
use bb_api::BitbucketClient;
use bb_core::{AuthError, Context};
use clap::Args;

use super::render;
use crate::auth;

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Filter by state
    #[arg(long, default_value = "OPEN", value_parser = ["OPEN", "MERGED", "DECLINED", "SUPERSEDED"])]
    pub state: String,
    /// Maximum number of pull requests to list
    #[arg(long, short = 'L', default_value_t = 30)]
    pub limit: usize,
    /// Filter by destination (base) branch
    #[arg(long)]
    pub base: Option<String>,
}

/// Run `bb pr list`.
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
    let pagelen = args.limit.clamp(1, 50);
    let mut path = format!(
        "/repositories/{}/{}/pullrequests?state={}&pagelen={pagelen}",
        repo.workspace(),
        repo.slug(),
        args.state,
    );
    if let Some(base) = &args.base {
        path.push_str(&format!(
            "&q={}",
            urlencode_query(&format!("destination.branch.name=\"{base}\""))
        ));
    }

    let prs: Vec<PullRequest> = client.paginate(&path, Some(args.limit))?;

    if prs.is_empty() {
        ctx.io.println(&format!(
            "No pull requests match your search in {}/{}.",
            repo.workspace(),
            repo.slug()
        ));
        return Ok(());
    }

    if ctx.io.is_stdout_tty() {
        ctx.io
            .print(&render::render_table(&prs, ctx.io.color_scheme()));
    } else {
        ctx.io.print(&render::render_tsv(&prs));
    }
    Ok(())
}

/// Percent-encode a `q=` value (spaces, quotes, `=`, ...).
fn urlencode_query(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
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
            state: "OPEN".to_owned(),
            limit: 30,
            base: None,
        }
    }

    const TWO_PRS: &str = r#"{
        "values": [
            {"id": 7, "title": "Fix bug", "state": "OPEN",
             "source": {"branch": {"name": "fix/x"}},
             "destination": {"branch": {"name": "main"}}},
            {"id": 9, "title": "Add feature", "state": "OPEN",
             "source": {"branch": {"name": "feat/y"}},
             "destination": {"branch": {"name": "main"}}}
        ]
    }"#;

    #[test]
    fn list_tsv_when_not_tty() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list",
            FakeTransport::rest(Method::Get, "/pullrequests"),
            FakeTransport::json(200, TWO_PRS),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, bufs) = test_context(transport, git(), config(), prompter, false);

        run(&ctx, list_args()).unwrap();

        let out = bufs.stdout_string();
        assert_eq!(
            out,
            "7\tFix bug\tfix/x->main\tOPEN\n9\tAdd feature\tfeat/y->main\tOPEN\n"
        );
        assert!(!out.contains("ID"));
    }

    #[test]
    fn list_table_when_tty() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list",
            FakeTransport::rest(Method::Get, "/pullrequests"),
            FakeTransport::json(200, TWO_PRS),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, bufs) = test_context(transport, git(), config(), prompter, true);

        run(&ctx, list_args()).unwrap();

        let out = bufs.stdout_string();
        let first = out.lines().next().unwrap();
        assert!(first.contains("ID"));
        assert!(first.contains("TITLE"));
        assert!(out.contains("#7"));
        assert!(out.contains("#9"));
    }

    #[test]
    fn list_empty_prints_message() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list empty",
            FakeTransport::rest(Method::Get, "/pullrequests"),
            FakeTransport::json(200, r#"{"values": []}"#),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, bufs) = test_context(transport, git(), config(), prompter, false);

        run(&ctx, list_args()).unwrap();
        assert!(bufs
            .stdout_string()
            .contains("No pull requests match your search in acme/widgets."));
    }

    #[test]
    fn list_base_adds_query_filter() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list base",
            FakeTransport::rest(Method::Get, "/pullrequests"),
            FakeTransport::json(200, r#"{"values": []}"#),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, _bufs) = test_context(transport, git(), config(), prompter, false);

        let a = ListArgs {
            base: Some("main".to_owned()),
            ..list_args()
        };
        run(&ctx, a).unwrap();

        let reqs = h.requests.lock().unwrap();
        let url = &reqs[0].url;
        assert!(url.contains("state=OPEN"));
        assert!(url.contains("q="));
        assert!(
            url.contains("destination.branch.name") && url.contains("main"),
            "url: {url}"
        );
    }

    #[test]
    fn list_pagelen_clamped_to_limit() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list small limit",
            FakeTransport::rest(Method::Get, "/pullrequests"),
            FakeTransport::json(200, r#"{"values": []}"#),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, _bufs) = test_context(transport, git(), config(), prompter, false);

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
    fn list_not_logged_in_is_auth_error() {
        let h = Arc::new(FakeTransport::new());
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let cfg: Arc<dyn ConfigProvider> = Arc::new(FileConfig::blank());
        let (ctx, _bufs) = test_context(transport, git(), cfg, prompter, false);

        let err = run(&ctx, list_args()).unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some());
    }
}
