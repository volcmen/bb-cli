//! `bb search` — search repositories, code, and pull requests.

use crate::api::models::{PullRequest, Repository};
use crate::api::BitbucketClient;
use crate::core::{AuthError, Context};
use crate::render::percent_encode;
use clap::{Args, Subcommand};

#[derive(Args, Debug)]
pub struct SearchArgs {
    #[command(subcommand)]
    command: SearchCommands,
}

#[derive(Subcommand, Debug)]
enum SearchCommands {
    /// Search repositories by name
    Repos(WsQuery),
    /// Search source code
    Code(WsQuery),
    /// Search pull requests in the current repo by title
    Prs(Query),
}

#[derive(Args, Debug)]
pub struct WsQuery {
    /// Search query
    #[arg(value_name = "QUERY")]
    pub query: String,
    /// Workspace to search (default: the current repo's workspace)
    #[arg(long, value_name = "WS")]
    pub workspace: Option<String>,
    /// Maximum number of results
    #[arg(long, short = 'L', default_value_t = 30)]
    pub limit: usize,
}

#[derive(Args, Debug)]
pub struct Query {
    /// Search query
    #[arg(value_name = "QUERY")]
    pub query: String,
    /// Maximum number of results
    #[arg(long, short = 'L', default_value_t = 30)]
    pub limit: usize,
}

/// One `search/code` result (we only need the file path).
#[derive(serde::Deserialize)]
struct CodeResult {
    file: Option<CodeFile>,
}
#[derive(serde::Deserialize)]
struct CodeFile {
    #[serde(default)]
    path: Option<String>,
}

/// Run `bb search <sub>`.
///
/// # Errors
/// [`AuthError`] (4) unauthenticated; propagates [`ApiError`](crate::core::ApiError).
pub fn run(ctx: &Context, args: SearchArgs) -> anyhow::Result<()> {
    let host = ctx.host();
    let Some(header) = crate::auth::header_for(ctx.config.as_ref(), &host) else {
        return Err(AuthError::new(host).into());
    };
    let client = BitbucketClient::new(ctx.transport.clone(), Some(header));

    match args.command {
        SearchCommands::Repos(a) => repos(ctx, &client, &a),
        SearchCommands::Code(a) => code(ctx, &client, &a),
        SearchCommands::Prs(a) => prs(ctx, &client, &a),
    }
}

/// `--workspace`, else the current repo's workspace.
fn workspace(ctx: &Context, explicit: &Option<String>) -> anyhow::Result<String> {
    match explicit {
        Some(ws) => Ok(ws.clone()),
        None => Ok(ctx.base_repo()?.workspace().to_owned()),
    }
}

fn repos(ctx: &Context, client: &BitbucketClient, args: &WsQuery) -> anyhow::Result<()> {
    let ws = workspace(ctx, &args.workspace)?;
    let pagelen = args.limit.clamp(1, 50);
    let q = percent_encode(&format!("name~\"{}\"", args.query));
    let path = format!("/repositories/{ws}?q={q}&pagelen={pagelen}");
    let hits: Vec<Repository> = client.paginate(&path, Some(args.limit))?;
    if hits.is_empty() {
        ctx.io.println("No repositories match.");
        return Ok(());
    }
    for r in hits {
        let name = r.full_name.as_deref().unwrap_or_default();
        let desc = r.description.as_deref().unwrap_or_default();
        ctx.io.println(&format!("{name}\t{desc}"));
    }
    Ok(())
}

fn code(ctx: &Context, client: &BitbucketClient, args: &WsQuery) -> anyhow::Result<()> {
    // Bitbucket's only code-search endpoint is `/workspaces/{ws}/search/code`,
    // which is workspace-wide — there is no documented `repo:` query qualifier
    // to narrow it to one repository. So when the workspace was inferred from a
    // specific repo (`-R WORKSPACE/SLUG`, or a git remote), the slug can't scope
    // the search; warn the user instead of silently dropping it.
    let ws = match &args.workspace {
        Some(ws) => ws.clone(),
        None => {
            let repo = ctx.base_repo()?;
            ctx.io.eprintln(&format!(
                "note: code search is workspace-wide on Bitbucket; the repo in -R only scopes the workspace ({})",
                repo.workspace()
            ));
            repo.workspace().to_owned()
        }
    };
    let pagelen = args.limit.clamp(1, 50);
    let sq = percent_encode(&args.query);
    let path = format!("/workspaces/{ws}/search/code?search_query={sq}&pagelen={pagelen}");
    let hits: Vec<CodeResult> = client.paginate(&path, Some(args.limit))?;
    let paths: Vec<String> = hits
        .into_iter()
        .filter_map(|h| h.file.and_then(|f| f.path))
        .collect();
    if paths.is_empty() {
        ctx.io.println("No code matches.");
        return Ok(());
    }
    for p in paths {
        ctx.io.println(&p);
    }
    Ok(())
}

fn prs(ctx: &Context, client: &BitbucketClient, args: &Query) -> anyhow::Result<()> {
    let repo = ctx.base_repo()?;
    let pagelen = args.limit.clamp(1, 50);
    let q = percent_encode(&format!("title~\"{}\"", args.query));
    let path = format!(
        "/repositories/{}/{}/pullrequests?q={q}&pagelen={pagelen}",
        repo.workspace(),
        repo.slug()
    );
    let hits: Vec<PullRequest> = client.paginate(&path, Some(args.limit))?;
    if hits.is_empty() {
        ctx.io.println("No pull requests match.");
        return Ok(());
    }
    for pr in hits {
        let title = pr.title.as_deref().unwrap_or_default();
        ctx.io.println(&format!("#{}\t{title}", pr.id));
    }
    Ok(())
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

    fn ctx_with(
        http: Arc<FakeTransport>,
        config: Arc<dyn ConfigProvider>,
    ) -> (Context, crate::core::TestBuffers) {
        let transport: Arc<dyn Transport> = http;
        let (mut ctx, bufs) = test_context(
            transport,
            git(),
            config,
            Arc::new(ScriptedPrompter::new()),
            false,
        );
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));
        (ctx, bufs)
    }

    #[test]
    fn repos_search_builds_query_and_lists() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "repos",
            FakeTransport::rest(Method::Get, "/repositories/myws"),
            FakeTransport::json(
                200,
                r#"{"values":[{"full_name":"myws/widget","description":"a widget"}]}"#,
            ),
        );
        let (ctx, bufs) = ctx_with(h.clone(), authed_config());
        run(
            &ctx,
            SearchArgs {
                command: SearchCommands::Repos(WsQuery {
                    query: "widget".to_owned(),
                    workspace: Some("myws".to_owned()),
                    limit: 30,
                }),
            },
        )
        .unwrap();
        assert!(bufs.stdout_string().contains("myws/widget\ta widget"));
        let reqs = h.requests.lock().unwrap();
        assert!(
            reqs[0].url.contains(&percent_encode("name~\"widget\"")),
            "url: {}",
            reqs[0].url
        );
    }

    #[test]
    fn code_search_lists_paths() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "code",
            FakeTransport::rest(Method::Get, "/workspaces/myws/search/code"),
            FakeTransport::json(
                200,
                r#"{"values":[{"file":{"path":"src/main.rs"}},{"file":{"path":"README.md"}}]}"#,
            ),
        );
        let (ctx, bufs) = ctx_with(h.clone(), authed_config());
        run(
            &ctx,
            SearchArgs {
                command: SearchCommands::Code(WsQuery {
                    query: "fn main".to_owned(),
                    workspace: Some("myws".to_owned()),
                    limit: 30,
                }),
            },
        )
        .unwrap();
        let out = bufs.stdout_string();
        assert!(out.contains("src/main.rs"), "out: {out}");
        assert!(out.contains("README.md"), "out: {out}");
    }

    #[test]
    fn code_search_without_workspace_warns_repo_only_scopes_workspace() {
        // No `--workspace`: the workspace is inferred from `-R acme/widgets`, so
        // the search runs workspace-wide on `acme` and the slug is dropped. The
        // command must emit a note saying so, and still query the `acme` ws.
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "code",
            FakeTransport::rest(Method::Get, "/workspaces/acme/search/code"),
            FakeTransport::json(200, r#"{"values":[{"file":{"path":"src/lib.rs"}}]}"#),
        );
        let (ctx, bufs) = ctx_with(h.clone(), authed_config());
        run(
            &ctx,
            SearchArgs {
                command: SearchCommands::Code(WsQuery {
                    query: "fn".to_owned(),
                    workspace: None,
                    limit: 30,
                }),
            },
        )
        .unwrap();
        assert!(bufs.stdout_string().contains("src/lib.rs"));
        let err = bufs.stderr_string();
        assert!(err.contains("workspace-wide"), "stderr: {err}");
        assert!(err.contains("(acme)"), "stderr: {err}");
    }

    #[test]
    fn code_search_with_explicit_workspace_emits_no_note() {
        // When `--workspace` is given the user already knows it's ws-scoped, so
        // no note should be printed.
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "code",
            FakeTransport::rest(Method::Get, "/workspaces/myws/search/code"),
            FakeTransport::json(200, r#"{"values":[{"file":{"path":"a.rs"}}]}"#),
        );
        let (ctx, bufs) = ctx_with(h.clone(), authed_config());
        run(
            &ctx,
            SearchArgs {
                command: SearchCommands::Code(WsQuery {
                    query: "fn".to_owned(),
                    workspace: Some("myws".to_owned()),
                    limit: 30,
                }),
            },
        )
        .unwrap();
        assert!(bufs.stderr_string().is_empty(), "unexpected note on stderr");
    }

    #[test]
    fn prs_search_lists() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "prs",
            FakeTransport::rest(Method::Get, "/repositories/acme/widgets/pullrequests"),
            FakeTransport::json(200, r#"{"values":[{"id":7,"title":"Fix the bug"}]}"#),
        );
        let (ctx, bufs) = ctx_with(h.clone(), authed_config());
        run(
            &ctx,
            SearchArgs {
                command: SearchCommands::Prs(Query {
                    query: "bug".to_owned(),
                    limit: 30,
                }),
            },
        )
        .unwrap();
        assert!(bufs.stdout_string().contains("#7\tFix the bug"));
    }

    #[test]
    fn search_not_authed_is_auth_error() {
        let h = Arc::new(FakeTransport::new());
        let (ctx, _bufs) = ctx_with(h.clone(), Arc::new(FileConfig::blank()));
        let err = run(
            &ctx,
            SearchArgs {
                command: SearchCommands::Prs(Query {
                    query: "x".to_owned(),
                    limit: 30,
                }),
            },
        )
        .unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some(), "got: {err}");
    }
}
