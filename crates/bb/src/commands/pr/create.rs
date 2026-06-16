//! `bb pr create` — open a pull request for the current branch.

use crate::api::models::{PullRequest, Repository};
use crate::api::{BitbucketClient, Membership};
use crate::core::{AuthError, Context, FlagError, RepoId};
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
struct Reviewer {
    uuid: String,
}

#[derive(serde::Serialize)]
struct CreatePrBody<'a> {
    title: &'a str,
    source: Endpoint<'a>,
    destination: Endpoint<'a>,
    description: &'a str,
    close_source_branch: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    reviewers: Vec<Reviewer>,
}

/// Run `bb pr create`.
///
/// # Errors
/// Returns [`AuthError`] (exit 4) if not authenticated, [`FlagError`] for usage
/// errors (e.g. no title when non-interactive), and propagates
/// [`ApiError`](crate::core::ApiError) / IO errors.
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

    // Resolve any requested reviewers to UUIDs (one members fetch, on demand).
    let reviewers = resolve_reviewers(&client, &repo, &args.reviewer)?;

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
        reviewers,
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

/// Resolve `--reviewer` strings to workspace-member UUIDs.
///
/// Returns an empty vec without any HTTP round-trip when no reviewers were
/// requested. Otherwise fetches the workspace member list once and matches each
/// requested string (case-insensitively) against any of a member's `uuid`,
/// `account_id`, `username`, `nickname`, or `display_name`.
///
/// # Errors
/// Returns [`FlagError`] naming every requested reviewer that did not match a
/// member (or matched a member that has no UUID). Propagates API errors from the
/// member fetch.
fn resolve_reviewers(
    client: &BitbucketClient,
    repo: &RepoId,
    requested: &[String],
) -> anyhow::Result<Vec<Reviewer>> {
    if requested.is_empty() {
        return Ok(Vec::new());
    }

    let path = format!("/workspaces/{}/members", repo.workspace());
    let members: Vec<Membership> = client.paginate::<Membership>(&path, None)?;

    let mut resolved = Vec::with_capacity(requested.len());
    let mut unresolved = Vec::new();

    for want in requested {
        match members
            .iter()
            .filter_map(|m| m.user.as_ref())
            .find(|u| member_matches(u, want))
            .and_then(|u| u.uuid.clone())
        {
            Some(uuid) => resolved.push(Reviewer { uuid }),
            None => unresolved.push(want.clone()),
        }
    }

    if !unresolved.is_empty() {
        return Err(FlagError::new(format!(
            "could not resolve reviewer(s): {}. Each must be a member of workspace {} \
             (matched by username, nickname, or account id).",
            unresolved.join(", "),
            repo.workspace(),
        ))
        .into());
    }

    Ok(resolved)
}

/// Whether `user` matches the requested reviewer string on any identity field,
/// compared case-insensitively.
fn member_matches(user: &crate::api::User, want: &str) -> bool {
    [
        user.uuid.as_deref(),
        user.account_id.as_deref(),
        user.username.as_deref(),
        user.nickname.as_deref(),
        user.display_name.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|field| field.eq_ignore_ascii_case(want))
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

fn to_anyhow(err: crate::core::PromptError) -> anyhow::Error {
    match err {
        crate::core::PromptError::Cancelled => crate::core::CancelError.into(),
        other => anyhow::anyhow!(other),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::api::testing::FakeTransport;
    use crate::config::FileConfig;
    use crate::core::{ConfigProvider, GitClient, Method, Transport};
    use crate::git::{ShellGit, StubRunner};

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
        let api = err
            .downcast_ref::<crate::core::ApiError>()
            .expect("ApiError");
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

    #[test]
    fn create_resolves_reviewers_to_uuids() {
        let h = Arc::new(FakeTransport::new());
        // The members list is fetched first (only because reviewers were requested).
        h.stub(
            "list members",
            FakeTransport::rest(Method::Get, "/workspaces/acme/members"),
            FakeTransport::json(
                200,
                r#"{"values": [
                    {"user": {"nickname": "alice", "uuid": "{a}"}},
                    {"user": {"nickname": "bob", "uuid": "{b}"}}
                ]}"#,
            ),
        );
        h.stub(
            "create pr",
            FakeTransport::rest(Method::Post, "/pullrequests"),
            FakeTransport::json(
                201,
                r#"{"id": 7, "links": {"html": {"href": "https://bitbucket.org/acme/widgets/pull-requests/7"}}}"#,
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
            title: Some("Add widget".to_owned()),
            base: Some("main".to_owned()),
            reviewer: vec!["alice".to_owned(), "bob".to_owned()],
            ..create_args()
        };
        run(&ctx, a).unwrap();

        let reqs = h.requests.lock().unwrap();
        let post = reqs
            .iter()
            .find(|r| r.method == Method::Post)
            .expect("a POST");
        let body: serde_json::Value =
            serde_json::from_slice(post.body.as_deref().unwrap()).unwrap();
        assert_eq!(
            body["reviewers"],
            serde_json::json!([{"uuid": "{a}"}, {"uuid": "{b}"}])
        );
    }

    #[test]
    fn create_unknown_reviewer_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        // The members list lacks the requested reviewer; the create POST is never
        // made, so it is intentionally NOT stubbed.
        h.stub(
            "list members",
            FakeTransport::rest(Method::Get, "/workspaces/acme/members"),
            FakeTransport::json(
                200,
                r#"{"values": [{"user": {"nickname": "alice", "uuid": "{a}"}}]}"#,
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
            title: Some("Add widget".to_owned()),
            base: Some("main".to_owned()),
            reviewer: vec!["ghost".to_owned()],
            ..create_args()
        };
        let err = run(&ctx, a).unwrap_err();
        let flag = err.downcast_ref::<FlagError>().expect("FlagError");
        assert!(
            flag.to_string().contains("ghost"),
            "error should name the unresolved reviewer: {flag}"
        );

        // No create POST was made.
        let reqs = h.requests.lock().unwrap();
        assert!(
            reqs.iter().all(|r| r.method != Method::Post),
            "create POST must not be made when a reviewer is unresolved"
        );
    }
}
