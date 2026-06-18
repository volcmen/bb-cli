//! `bb pr edit` — update a pull request's title, description, or base branch.

use crate::api::{BitbucketClient, PullRequest};
use crate::core::{AuthError, Context, FlagError};
use clap::Args;

#[derive(Args, Debug)]
pub struct EditArgs {
    /// Pull request id (defaults to the PR for the current branch)
    #[arg(value_name = "ID")]
    pub id: Option<String>,
    /// New title
    #[arg(long, short = 't')]
    pub title: Option<String>,
    /// New description/body
    #[arg(long, short = 'b')]
    pub body: Option<String>,
    /// Read the new description from a file (use "-" for stdin)
    #[arg(long = "body-file", short = 'F', value_name = "FILE")]
    pub body_file: Option<String>,
    /// New base (destination) branch
    #[arg(long, short = 'B')]
    pub base: Option<String>,
    /// Add a reviewer (username/nickname/account id/uuid; repeatable, comma-ok)
    #[arg(long = "add-reviewer", value_delimiter = ',')]
    pub add_reviewer: Vec<String>,
    /// Remove a reviewer (matched on the PR's current reviewers; repeatable)
    #[arg(long = "remove-reviewer", value_delimiter = ',')]
    pub remove_reviewer: Vec<String>,
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

/// The JSON body sent to `PUT .../pullrequests/{id}`. Bitbucket replaces the PR
/// object, so we always send the (possibly unchanged) title/description/base.
/// `reviewers` is sent only when a `--add-reviewer`/`--remove-reviewer` was
/// requested; otherwise it is omitted so the existing reviewer set is preserved.
#[derive(serde::Serialize)]
struct EditPrBody<'a> {
    title: &'a str,
    description: &'a str,
    destination: Endpoint<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reviewers: Option<Vec<super::create::Reviewer>>,
}

/// Run `bb pr edit`.
///
/// # Errors
/// Returns [`AuthError`] (exit 4) when not authenticated, [`FlagError`] (exit 1)
/// when nothing was passed to update / the id is invalid / the PR is not found,
/// and propagates [`ApiError`](crate::core::ApiError) / IO errors.
pub fn run(ctx: &Context, args: EditArgs) -> anyhow::Result<()> {
    let repo = ctx.base_repo()?;
    let host = repo.host().to_owned();

    let Some(header) = crate::auth::header_for(ctx.config.as_ref(), &host) else {
        return Err(AuthError::new(host).into());
    };
    let client = BitbucketClient::new(ctx.transport.clone(), Some(header));

    // Require at least one change before touching the network.
    if args.title.is_none()
        && args.body.is_none()
        && args.body_file.is_none()
        && args.base.is_none()
        && args.add_reviewer.is_empty()
        && args.remove_reviewer.is_empty()
    {
        return Err(FlagError::new(
            "nothing to update; pass --title, --body/--body-file, --base, \
             --add-reviewer, or --remove-reviewer",
        )
        .into());
    }

    let id = match args.id.as_deref() {
        Some(s) => super::finder::parse_id(s)?,
        None => super::finder::resolve(ctx, &client, &repo, None)?.id,
    };

    let body_override = resolve_body(ctx, &args)?;

    let path = format!(
        "/repositories/{}/{}/pullrequests/{id}",
        repo.workspace(),
        repo.slug()
    );

    // Fetch the current PR so unspecified fields are preserved on the PUT.
    let current: PullRequest = match client.get(&path) {
        Ok(pr) => pr,
        Err(e) if e.is_not_found() => {
            return Err(FlagError::new(format!(
                "no pull request #{id} found in {}/{}",
                repo.workspace(),
                repo.slug()
            ))
            .into());
        }
        Err(e) => return Err(e.into()),
    };

    let title = args
        .title
        .as_deref()
        .or(current.title.as_deref())
        .unwrap_or_default();
    let description = body_override
        .as_deref()
        .or(current.description.as_deref())
        .unwrap_or_default();
    let base = args
        .base
        .as_deref()
        .or_else(|| current.destination.branch.as_ref().map(|b| b.name.as_str()))
        .unwrap_or_default();

    let reviewers = resolve_reviewer_edits(&client, &repo, &args, &current)?;

    let payload = EditPrBody {
        title,
        description,
        destination: Endpoint {
            branch: BranchName { name: base },
        },
        reviewers,
    };
    let updated: PullRequest = client.put(&path, &payload)?;

    let url = updated.html_url().map_or_else(
        || {
            format!(
                "https://bitbucket.org/{}/{}/pull-requests/{id}",
                repo.workspace(),
                repo.slug()
            )
        },
        ToOwned::to_owned,
    );
    ctx.io.println(&format!("✓ Updated pull request #{id}"));
    ctx.io.println(&url);
    Ok(())
}

/// Build the merged reviewer set when `--add-reviewer`/`--remove-reviewer` were
/// passed, else `None` (so `reviewers` is omitted from the PUT and the current set
/// is preserved).
///
/// Removals are matched against the PR's *current* reviewer objects (so a member
/// who left the workspace can still be removed). Additions are resolved against the
/// workspace member list (only fetched when there is something to add). Remove is
/// applied before add, so re-adding a just-removed reviewer keeps them.
///
/// # Errors
/// Returns [`FlagError`] when an `--add-reviewer` cannot be resolved to a member.
fn resolve_reviewer_edits(
    client: &BitbucketClient,
    repo: &crate::core::RepoId,
    args: &EditArgs,
    current: &PullRequest,
) -> anyhow::Result<Option<Vec<super::create::Reviewer>>> {
    if args.add_reviewer.is_empty() && args.remove_reviewer.is_empty() {
        return Ok(None);
    }

    // Current reviewers, minus any matched by --remove-reviewer.
    let mut uuids: Vec<String> = current
        .reviewers
        .iter()
        .filter(|u| {
            !args
                .remove_reviewer
                .iter()
                .any(|want| super::create::member_matches(u, want))
        })
        .filter_map(|u| u.uuid.clone())
        .collect();

    // Resolve additions to UUIDs and append, deduping by uuid.
    for reviewer in super::create::resolve_reviewers(client, repo, &args.add_reviewer)? {
        if !uuids.contains(&reviewer.uuid) {
            uuids.push(reviewer.uuid);
        }
    }

    Ok(Some(
        uuids
            .into_iter()
            .map(|uuid| super::create::Reviewer { uuid })
            .collect(),
    ))
}

/// Resolve the new description from `--body`, then `--body-file` (`-` => stdin),
/// else `None` (keep the current description).
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

    fn ctx_with(http: Arc<FakeTransport>) -> (Context, crate::core::TestBuffers) {
        let transport: Arc<dyn Transport> = http;
        let (mut ctx, bufs) = test_context(
            transport,
            git(),
            authed_config(),
            Arc::new(ScriptedPrompter::new()),
            false,
        );
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));
        (ctx, bufs)
    }

    const CURRENT_PR: &str = r#"{
        "id": 42,
        "title": "Old title",
        "description": "old body",
        "destination": {"branch": {"name": "main"}},
        "links": {"html": {"href": "https://bitbucket.org/acme/widgets/pull-requests/42"}}
    }"#;

    fn stub_get(h: &Arc<FakeTransport>) {
        h.stub(
            "get pr 42",
            FakeTransport::rest(Method::Get, "/pullrequests/42"),
            FakeTransport::json(200, CURRENT_PR),
        );
    }

    fn stub_put(h: &Arc<FakeTransport>) {
        h.stub(
            "put pr 42",
            FakeTransport::rest(Method::Put, "/pullrequests/42"),
            FakeTransport::json(200, CURRENT_PR),
        );
    }

    fn args() -> EditArgs {
        EditArgs {
            id: Some("42".to_owned()),
            title: None,
            body: None,
            body_file: None,
            base: None,
            add_reviewer: Vec::new(),
            remove_reviewer: Vec::new(),
        }
    }

    fn put_body(h: &FakeTransport) -> serde_json::Value {
        let reqs = h.requests.lock().unwrap();
        let put = reqs
            .iter()
            .find(|r| r.method == Method::Put)
            .expect("a PUT request");
        serde_json::from_slice(put.body.as_deref().unwrap()).unwrap()
    }

    #[test]
    fn edit_updates_title_and_preserves_other_fields() {
        let h = Arc::new(FakeTransport::new());
        stub_get(&h);
        stub_put(&h);
        let (ctx, bufs) = ctx_with(h.clone());

        let a = EditArgs {
            title: Some("New title".to_owned()),
            ..args()
        };
        run(&ctx, a).unwrap();

        let body = put_body(&h);
        assert_eq!(body["title"], "New title");
        // Unchanged fields preserved from the fetched PR.
        assert_eq!(body["description"], "old body");
        assert_eq!(body["destination"]["branch"]["name"], "main");
        assert!(bufs.stdout_string().contains("✓ Updated pull request #42"));
    }

    #[test]
    fn edit_base_changes_destination() {
        let h = Arc::new(FakeTransport::new());
        stub_get(&h);
        stub_put(&h);
        let (ctx, _bufs) = ctx_with(h.clone());

        let a = EditArgs {
            base: Some("develop".to_owned()),
            ..args()
        };
        run(&ctx, a).unwrap();

        let body = put_body(&h);
        assert_eq!(body["destination"]["branch"]["name"], "develop");
        // Title preserved.
        assert_eq!(body["title"], "Old title");
    }

    #[test]
    fn edit_body_override_changes_description() {
        let h = Arc::new(FakeTransport::new());
        stub_get(&h);
        stub_put(&h);
        let (ctx, _bufs) = ctx_with(h.clone());

        let a = EditArgs {
            body: Some("a new description".to_owned()),
            ..args()
        };
        run(&ctx, a).unwrap();

        let body = put_body(&h);
        assert_eq!(body["description"], "a new description");
        // Title preserved from the fetched PR.
        assert_eq!(body["title"], "Old title");
    }

    #[test]
    fn edit_body_file_reads_file() {
        let h = Arc::new(FakeTransport::new());
        stub_get(&h);
        stub_put(&h);
        let (ctx, _bufs) = ctx_with(h.clone());

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("body.md");
        std::fs::write(&file, "from a file").unwrap();

        let a = EditArgs {
            body_file: Some(file.to_string_lossy().into_owned()),
            ..args()
        };
        run(&ctx, a).unwrap();

        let body = put_body(&h);
        assert_eq!(body["description"], "from a file");
    }

    #[test]
    fn edit_no_fields_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        let (ctx, _bufs) = ctx_with(h.clone());

        let err = run(&ctx, args()).unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
        assert_eq!(h.request_count(), 0);
    }

    #[test]
    fn edit_not_authed_is_auth_error() {
        let h = Arc::new(FakeTransport::new());
        let transport: Arc<dyn Transport> = h.clone();
        let (mut ctx, _bufs) = test_context(
            transport,
            git(),
            Arc::new(FileConfig::blank()),
            Arc::new(ScriptedPrompter::new()),
            false,
        );
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));

        let a = EditArgs {
            title: Some("x".to_owned()),
            ..args()
        };
        let err = run(&ctx, a).unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some(), "got: {err}");
    }

    // ----- reviewer editing (#117) ---------------------------------------

    fn stub_get_body(h: &Arc<FakeTransport>, body: &'static str) {
        h.stub(
            "get pr 42",
            FakeTransport::rest(Method::Get, "/pullrequests/42"),
            FakeTransport::json(200, body),
        );
    }

    fn stub_members(h: &Arc<FakeTransport>) {
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
    }

    const PR_ONE_REVIEWER: &str = r#"{
        "id": 42, "title": "T", "description": "d",
        "destination": {"branch": {"name": "main"}},
        "reviewers": [{"nickname": "alice", "uuid": "{a}"}]
    }"#;

    const PR_TWO_REVIEWERS: &str = r#"{
        "id": 42, "title": "T", "description": "d",
        "destination": {"branch": {"name": "main"}},
        "reviewers": [
            {"nickname": "alice", "uuid": "{a}"},
            {"nickname": "bob", "uuid": "{b}"}
        ]
    }"#;

    fn reviewer_uuids(h: &FakeTransport) -> Vec<String> {
        let body = put_body(h);
        body["reviewers"]
            .as_array()
            .expect("reviewers array in PUT body")
            .iter()
            .map(|r| r["uuid"].as_str().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn edit_add_reviewer_merges_into_current() {
        let h = Arc::new(FakeTransport::new());
        stub_get_body(&h, PR_ONE_REVIEWER);
        stub_members(&h);
        stub_put(&h);
        let (ctx, _bufs) = ctx_with(h.clone());

        let a = EditArgs {
            add_reviewer: vec!["bob".to_owned()],
            ..args()
        };
        run(&ctx, a).unwrap();

        assert_eq!(reviewer_uuids(&h), vec!["{a}", "{b}"]);
    }

    #[test]
    fn edit_remove_reviewer_drops_from_current() {
        let h = Arc::new(FakeTransport::new());
        // No members fetch: removal matches the PR's own reviewer objects.
        stub_get_body(&h, PR_TWO_REVIEWERS);
        stub_put(&h);
        let (ctx, _bufs) = ctx_with(h.clone());

        let a = EditArgs {
            remove_reviewer: vec!["alice".to_owned()],
            ..args()
        };
        run(&ctx, a).unwrap();

        assert_eq!(reviewer_uuids(&h), vec!["{b}"]);
    }

    #[test]
    fn edit_add_and_remove_compose() {
        let h = Arc::new(FakeTransport::new());
        stub_get_body(&h, PR_ONE_REVIEWER);
        stub_members(&h);
        stub_put(&h);
        let (ctx, _bufs) = ctx_with(h.clone());

        let a = EditArgs {
            add_reviewer: vec!["bob".to_owned()],
            remove_reviewer: vec!["alice".to_owned()],
            ..args()
        };
        run(&ctx, a).unwrap();

        assert_eq!(reviewer_uuids(&h), vec!["{b}"]);
    }

    #[test]
    fn edit_add_unknown_reviewer_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        // GET + members are hit; the PUT is never reached, so it is not stubbed.
        stub_get_body(&h, PR_ONE_REVIEWER);
        stub_members(&h);
        let (ctx, _bufs) = ctx_with(h.clone());

        let a = EditArgs {
            add_reviewer: vec!["carol".to_owned()],
            ..args()
        };
        let err = run(&ctx, a).unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
    }

    #[test]
    fn edit_reviewer_flag_satisfies_change_guard() {
        // Only --remove-reviewer (no title/body/base) still reaches the network.
        let h = Arc::new(FakeTransport::new());
        stub_get_body(&h, PR_TWO_REVIEWERS);
        stub_put(&h);
        let (ctx, _bufs) = ctx_with(h.clone());

        let a = EditArgs {
            remove_reviewer: vec!["bob".to_owned()],
            ..args()
        };
        run(&ctx, a).unwrap();
        assert_eq!(reviewer_uuids(&h), vec!["{a}"]);
    }
}
