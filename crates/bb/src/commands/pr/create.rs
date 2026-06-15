//! `bb pr create` — open a pull request for the current branch.

use bb_api::models::{PullRequest, Repository};
use bb_api::BitbucketClient;
use bb_core::{AuthError, Context, FlagError, RepoId};
use clap::Args;

use crate::auth;

#[derive(Args, Debug)]
pub struct CreateArgs {
    /// Title for the pull request
    #[arg(long, short)]
    pub title: Option<String>,
    /// Body/description for the pull request
    #[arg(long, short)]
    pub body: Option<String>,
    /// Read the body from a file (use "-" for stdin)
    #[arg(long = "body-file", short = 'F', value_name = "FILE")]
    pub body_file: Option<String>,
    /// The destination (base) branch (default: repo main branch)
    #[arg(long, short = 'B')]
    pub base: Option<String>,
    /// The source (head) branch (default: current branch)
    #[arg(long, short = 'H')]
    pub head: Option<String>,
    /// Close the source branch after merge
    #[arg(long)]
    pub close_source_branch: bool,
    /// Open the new PR in the browser instead of creating via API
    #[arg(long)]
    pub web: bool,
    /// Request reviewers (comma-separated usernames)
    #[arg(long, value_delimiter = ',')]
    pub reviewer: Vec<String>,
}

// ----- request body shapes ----------------------------------------------

#[derive(serde::Serialize)]
struct BranchName<'a> {
    name: &'a str,
}

#[derive(serde::Serialize)]
struct Endpoint<'a> {
    branch: BranchName<'a>,
}

#[derive(serde::Serialize)]
struct CreatePrBody<'a> {
    title: &'a str,
    source: Endpoint<'a>,
    destination: Endpoint<'a>,
    description: &'a str,
    close_source_branch: bool,
}

/// Run `bb pr create`.
///
/// # Errors
/// Returns [`AuthError`] (exit 4) if not authenticated, [`FlagError`] for usage
/// errors (e.g. no title when non-interactive), and propagates
/// [`ApiError`](bb_core::ApiError) / IO errors.
pub fn run(ctx: &Context, args: CreateArgs) -> anyhow::Result<()> {
    let repo = ctx.base_repo()?;
    let host = repo.host().to_owned();

    let Some(header) = auth::header_for(ctx.config.as_ref(), &host) else {
        return Err(AuthError::new(host).into());
    };
    let client = BitbucketClient::new(ctx.transport.clone(), Some(header));

    // head = --head or current branch.
    let head = match &args.head {
        Some(h) => h.clone(),
        None => ctx.git.current_branch()?,
    };

    // base = --base, else repository main branch, else "main".
    let base = match &args.base {
        Some(b) => b.clone(),
        None => default_base(&client, &repo)?,
    };

    // Reviewers are not resolved until Epic 1.
    if !args.reviewer.is_empty() {
        ctx.io
            .eprintln("note: reviewer resolution lands in Epic 1; --reviewer is ignored for now.");
    }

    // --web short-circuits to the compare page.
    if args.web {
        let url = format!(
            "https://bitbucket.org/{}/{}/pull-requests/new?source={}&dest={}",
            repo.workspace(),
            repo.slug(),
            url_encode(&head),
            url_encode(&base),
        );
        let _ = ctx.browser.browse(&url);
        ctx.io.println(&url);
        return Ok(());
    }

    // body: --body, else --body-file, else "".
    let body = resolve_body(ctx, &args)?;

    // title: --title, else prompt (default head), else FlagError.
    let title = match args.title {
        Some(t) => t,
        None => {
            if ctx.io.can_prompt() {
                ctx.prompter
                    .input("Title", Some(&head))
                    .map_err(to_anyhow)?
            } else {
                return Err(
                    FlagError::new("--title required when not running interactively").into(),
                );
            }
        }
    };

    let payload = CreatePrBody {
        title: &title,
        source: Endpoint {
            branch: BranchName { name: &head },
        },
        destination: Endpoint {
            branch: BranchName { name: &base },
        },
        description: &body,
        close_source_branch: args.close_source_branch,
    };

    let path = format!(
        "/repositories/{}/{}/pullrequests",
        repo.workspace(),
        repo.slug()
    );
    let pr: PullRequest = client.post(&path, &payload)?;

    let url = pr.html_url().map_or_else(
        || {
            format!(
                "https://bitbucket.org/{}/{}/pull-requests/{}",
                repo.workspace(),
                repo.slug(),
                pr.id
            )
        },
        ToOwned::to_owned,
    );
    ctx.io.println(&url);
    Ok(())
}

/// Resolve the PR description from `--body`, then `--body-file` (`-` => stdin),
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

/// The repo's default base branch: its `mainbranch.name`, falling back to
/// `"main"` only when the GET succeeds but the field is absent. Transport/HTTP
/// errors are propagated rather than silently swallowed.
fn default_base(client: &BitbucketClient, repo: &RepoId) -> anyhow::Result<String> {
    let path = format!("/repositories/{}/{}", repo.workspace(), repo.slug());
    let repo: Repository = client.get(&path)?;
    Ok(repo
        .mainbranch
        .map_or_else(|| "main".to_owned(), |m| m.name))
}

/// Minimal percent-encoding for URL query/path values (used for the `--web`
/// compare URL). Matches the unreserved set; everything else is `%XX`.
fn url_encode(s: &str) -> String {
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
    use bb_core::{ConfigProvider, GitClient, Method, Transport};
    use bb_git::{ShellGit, StubRunner};

    use super::*;
    use crate::testsupport::{test_context, ScriptedPrompter};

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

    fn git_no_branch() -> Arc<dyn GitClient> {
        let s = Arc::new(StubRunner::new());
        s.register(
            "remote -v",
            0,
            "origin\tgit@bitbucket.org:acme/widgets.git (fetch)\n",
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

    fn create_args() -> CreateArgs {
        CreateArgs {
            title: None,
            body: None,
            body_file: None,
            base: None,
            head: None,
            close_source_branch: false,
            web: false,
            reviewer: Vec::new(),
        }
    }

    #[test]
    fn create_happy_path_posts_and_prints_url() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "create pr",
            FakeTransport::rest(Method::Post, "/pullrequests"),
            FakeTransport::json(
                201,
                r#"{"id": 42, "title": "Add widget", "state": "OPEN",
                    "source": {"branch": {"name": "feature/x"}},
                    "destination": {"branch": {"name": "main"}},
                    "links": {"html": {"href": "https://bitbucket.org/acme/widgets/pull-requests/42"}}}"#,
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

        let a = CreateArgs {
            title: Some("Add widget".to_owned()),
            base: Some("main".to_owned()),
            body: Some("the body".to_owned()),
            ..create_args()
        };
        run(&ctx, a).unwrap();

        // printed URL
        assert!(bufs
            .stdout_string()
            .contains("https://bitbucket.org/acme/widgets/pull-requests/42"));

        // POST body shape
        let reqs = h.requests.lock().unwrap();
        let post = reqs
            .iter()
            .find(|r| r.method == Method::Post)
            .expect("a POST");
        let body: serde_json::Value =
            serde_json::from_slice(post.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["title"], "Add widget");
        assert_eq!(body["source"]["branch"]["name"], "feature/x");
        assert_eq!(body["destination"]["branch"]["name"], "main");
        assert_eq!(body["description"], "the body");
        assert_eq!(body["close_source_branch"], false);
    }

    #[test]
    fn create_defaults_base_from_repo_mainbranch() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get repo",
            FakeTransport::rest(Method::Get, "/repositories/acme/widgets"),
            FakeTransport::json(200, r#"{"slug":"widgets","mainbranch":{"name":"develop"}}"#),
        );
        h.stub(
            "create pr",
            FakeTransport::rest(Method::Post, "/pullrequests"),
            FakeTransport::json(
                201,
                r#"{"id": 1, "links": {"html": {"href": "https://bitbucket.org/acme/widgets/pull-requests/1"}}}"#,
            ),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, _bufs) = test_context(
            transport,
            git_with_branch("feature/x"),
            config(),
            prompter,
            false,
        );

        let a = CreateArgs {
            title: Some("T".to_owned()),
            ..create_args()
        };
        run(&ctx, a).unwrap();

        let reqs = h.requests.lock().unwrap();
        let post = reqs.iter().find(|r| r.method == Method::Post).unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(post.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["destination"]["branch"]["name"], "develop");
    }

    #[test]
    fn create_propagates_repo_lookup_error() {
        // No --base: the repo GET is required. A server error must surface, not
        // silently default to "main".
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get repo 500",
            FakeTransport::rest(Method::Get, "/repositories/acme/widgets"),
            FakeTransport::json(500, r#"{"type":"error","error":{"message":"boom"}}"#),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, _bufs) = test_context(
            transport,
            git_with_branch("feature/x"),
            config(),
            prompter,
            false,
        );

        let a = CreateArgs {
            title: Some("T".to_owned()),
            ..create_args()
        };
        let err = run(&ctx, a).unwrap_err();
        let api = err.downcast_ref::<bb_core::ApiError>().expect("ApiError");
        assert_eq!(api.status(), Some(500));
    }

    #[test]
    fn create_non_interactive_missing_title_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        // tty=false => can_prompt() false. Pass base to avoid the repo GET.
        let (ctx, _bufs) = test_context(
            transport,
            git_with_branch("feature/x"),
            config(),
            prompter,
            false,
        );

        let a = CreateArgs {
            base: Some("main".to_owned()),
            ..create_args()
        };
        let err = run(&ctx, a).unwrap_err();
        let flag = err.downcast_ref::<FlagError>().expect("FlagError");
        assert!(flag.to_string().contains("--title required"));
    }

    #[test]
    fn create_not_logged_in_is_auth_error() {
        let h = Arc::new(FakeTransport::new());
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let cfg: Arc<dyn ConfigProvider> = Arc::new(FileConfig::blank());
        let (ctx, _bufs) = test_context(transport, git_no_branch(), cfg, prompter, false);

        let a = CreateArgs {
            title: Some("T".to_owned()),
            base: Some("main".to_owned()),
            ..create_args()
        };
        let err = run(&ctx, a).unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some());
    }

    #[test]
    fn create_web_opens_browser_and_prints_url() {
        let h = Arc::new(FakeTransport::new());
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, bufs) = test_context(
            transport,
            git_with_branch("feature/x"),
            config(),
            prompter,
            false,
        );

        let a = CreateArgs {
            web: true,
            base: Some("main".to_owned()),
            ..create_args()
        };
        run(&ctx, a).unwrap();

        let out = bufs.stdout_string();
        // head "feature/x" is percent-encoded (the '/' becomes %2F).
        assert!(
            out.contains(
                "https://bitbucket.org/acme/widgets/pull-requests/new?source=feature%2Fx&dest=main"
            ),
            "got: {out}"
        );
    }
}
