//! Shared pull-request resolution: by numeric id, or by inferring from the
//! current git branch (the analog of `gh`'s PR finder).

use bb_api::{BitbucketClient, PullRequest};
use bb_core::{Context, FlagError, RepoId};

/// Resolve a PR from an optional `selector`:
/// - `Some("123")` / `Some("#123")` → fetch by id.
/// - `None` → the most recent OPEN PR whose source branch is the current branch.
///
/// # Errors
/// `FlagError` for a malformed id; an error if no PR is found or the API fails.
pub(crate) fn resolve(
    ctx: &Context,
    client: &BitbucketClient,
    repo: &RepoId,
    selector: Option<&str>,
) -> anyhow::Result<PullRequest> {
    match selector {
        Some(sel) => {
            let id = parse_id(sel)?;
            find_by_id(client, repo, id)
        }
        None => {
            let branch = ctx.git.current_branch()?;
            find_by_branch(client, repo, &branch)
        }
    }
}

pub(crate) fn parse_id(selector: &str) -> Result<u64, FlagError> {
    selector
        .trim()
        .trim_start_matches('#')
        .parse::<u64>()
        .map_err(|_| FlagError::new(format!("invalid pull request id: {selector:?}")))
}

pub(crate) fn find_by_id(
    client: &BitbucketClient,
    repo: &RepoId,
    id: u64,
) -> anyhow::Result<PullRequest> {
    let path = format!(
        "/repositories/{}/{}/pullrequests/{id}",
        repo.workspace(),
        repo.slug()
    );
    client.get::<PullRequest>(&path).map_err(|e| {
        if e.is_not_found() {
            anyhow::anyhow!("no pull request #{id} found in {repo}")
        } else {
            e.into()
        }
    })
}

fn find_by_branch(
    client: &BitbucketClient,
    repo: &RepoId,
    branch: &str,
) -> anyhow::Result<PullRequest> {
    let q = encode_query(&format!("source.branch.name=\"{branch}\""));
    let path = format!(
        "/repositories/{}/{}/pullrequests?state=OPEN&q={q}",
        repo.workspace(),
        repo.slug()
    );
    let prs: Vec<PullRequest> = client.paginate(&path, Some(1))?;
    prs.into_iter().next().ok_or_else(|| {
        anyhow::anyhow!("no open pull request found for branch {branch:?}; pass a PR id")
    })
}

/// Percent-encode a query-string component.
fn encode_query(s: &str) -> String {
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

    fn ctx_with_branch(branch: &str, http: Arc<FakeTransport>) -> Context {
        let stub = Arc::new(StubRunner::new());
        stub.register("rev-parse --abbrev-ref HEAD", 0, &format!("{branch}\n"));
        let git: Arc<dyn GitClient> = Arc::new(ShellGit::new(stub));
        let config: Arc<dyn ConfigProvider> = Arc::new(FileConfig::blank());
        let transport: Arc<dyn Transport> = http;
        let (ctx, _bufs) = test_context(
            transport,
            git,
            config,
            Arc::new(ScriptedPrompter::new()),
            false,
        );
        ctx
    }

    /// A context whose git stub registers nothing (branch inference not called).
    fn ctx_no_branch(http: Arc<FakeTransport>) -> Context {
        let git: Arc<dyn GitClient> = Arc::new(ShellGit::new(Arc::new(StubRunner::new())));
        let config: Arc<dyn ConfigProvider> = Arc::new(FileConfig::blank());
        let transport: Arc<dyn Transport> = http;
        let (ctx, _bufs) = test_context(
            transport,
            git,
            config,
            Arc::new(ScriptedPrompter::new()),
            false,
        );
        ctx
    }

    #[test]
    fn parse_id_accepts_hash_prefix() {
        assert_eq!(parse_id("#42").unwrap(), 42);
        assert_eq!(parse_id("42").unwrap(), 42);
        assert!(parse_id("abc").is_err());
    }

    #[test]
    fn resolves_by_id() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "GET pr 42",
            FakeTransport::rest(Method::Get, "/pullrequests/42"),
            FakeTransport::json(200, r#"{"id":42,"title":"T","state":"OPEN"}"#),
        );
        let ctx = ctx_no_branch(h.clone());
        let client = BitbucketClient::new(h, None);
        let repo = RepoId::new("acme", "widgets");
        let pr = resolve(&ctx, &client, &repo, Some("42")).unwrap();
        assert_eq!(pr.id, 42);
    }

    #[test]
    fn resolves_by_current_branch() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list by branch",
            FakeTransport::rest(Method::Get, "/pullrequests?state=OPEN&q="),
            FakeTransport::json(200, r#"{"values":[{"id":7,"title":"T","state":"OPEN"}]}"#),
        );
        let ctx = ctx_with_branch("feature/x", h.clone());
        let client = BitbucketClient::new(h, None);
        let repo = RepoId::new("acme", "widgets");
        let pr = resolve(&ctx, &client, &repo, None).unwrap();
        assert_eq!(pr.id, 7);
    }

    #[test]
    fn errors_when_branch_has_no_pr() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list empty",
            FakeTransport::rest(Method::Get, "/pullrequests?state=OPEN&q="),
            FakeTransport::json(200, r#"{"values":[]}"#),
        );
        let ctx = ctx_with_branch("feature/x", h.clone());
        let client = BitbucketClient::new(h, None);
        let repo = RepoId::new("acme", "widgets");
        assert!(resolve(&ctx, &client, &repo, None).is_err());
    }
}
