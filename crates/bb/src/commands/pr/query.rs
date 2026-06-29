//! Render-free pull-request *fetch* helpers, shared by the CLI commands and (per
//! spec 033) the forthcoming TUI. These own path-building + pagination so callers
//! only resolve repo/auth, call a query fn, then render however they like.

use crate::api::models::{CommitStatus, PullRequest};
use crate::api::BitbucketClient;
use crate::core::{ApiError, RepoId};
use crate::render::{bbql_escape, percent_encode};

/// What pull requests to list — built from `pr list` flags or a TUI section.
#[derive(Debug, Clone)]
pub struct PrFilter {
    pub state: String,
    pub base: Option<String>,
    pub limit: usize,
}

impl PrFilter {
    /// The exact `pullrequests` query path. `pagelen` is clamped to Bitbucket's
    /// 1..=50 and the requested limit; `--base` folds the state into a BBQL `q`
    /// because Bitbucket ignores the standalone `state=` param when `q` is present
    /// (#114).
    pub(crate) fn path(&self, repo: &RepoId) -> String {
        let pagelen = self.limit.clamp(1, 50);
        let prefix = format!(
            "/repositories/{}/{}/pullrequests?pagelen={pagelen}",
            repo.workspace(),
            repo.slug(),
        );
        if let Some(branch) = &self.base {
            let q = format!(
                "state=\"{}\" AND destination.branch.name=\"{}\"",
                bbql_escape(&self.state),
                bbql_escape(branch)
            );
            format!("{prefix}&q={}", percent_encode(&q))
        } else {
            format!("{prefix}&state={}", self.state)
        }
    }
}

/// List pull requests matching `filter` (paginated up to `filter.limit`).
///
/// # Errors
/// Propagates [`ApiError`] from the listing call.
pub fn list(
    client: &BitbucketClient,
    repo: &RepoId,
    filter: &PrFilter,
) -> Result<Vec<PullRequest>, ApiError> {
    client.paginate(&filter.path(repo), Some(filter.limit))
}

/// Fetch a single pull request by id.
///
/// # Errors
/// An error if the PR is not found or the API call fails.
pub fn get(client: &BitbucketClient, repo: &RepoId, id: u64) -> anyhow::Result<PullRequest> {
    super::finder::find_by_id(client, repo, id)
}

/// Fetch the commit build statuses ("checks") for a commit `sha`.
///
/// # Errors
/// Propagates [`ApiError`] from the statuses call.
pub fn checks(
    client: &BitbucketClient,
    repo: &RepoId,
    sha: &str,
) -> Result<Vec<CommitStatus>, ApiError> {
    let path = format!(
        "/repositories/{}/{}/commit/{sha}/statuses",
        repo.workspace(),
        repo.slug(),
    );
    client.paginate(&path, None)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::api::testing::FakeTransport;
    use crate::core::{Method, RepoId};

    use super::*;

    fn repo() -> RepoId {
        RepoId::new("acme", "widgets")
    }

    #[test]
    fn filter_path_default_uses_state_param() {
        let f = PrFilter {
            state: "OPEN".to_owned(),
            base: None,
            limit: 30,
        };
        assert_eq!(
            f.path(&repo()),
            "/repositories/acme/widgets/pullrequests?pagelen=30&state=OPEN"
        );
    }

    #[test]
    fn filter_path_clamps_pagelen_and_folds_base_into_q() {
        let f = PrFilter {
            state: "MERGED".to_owned(),
            base: Some("main".to_owned()),
            limit: 100,
        };
        let path = f.path(&repo());
        assert!(path.contains("pagelen=50"), "path: {path}");
        let q = percent_encode(r#"state="MERGED" AND destination.branch.name="main""#);
        assert!(path.contains(&format!("&q={q}")), "path: {path}");
        assert!(!path.contains("&state="), "path: {path}");
    }

    #[test]
    fn filter_path_escapes_quotes_in_base_branch() {
        // A base branch containing a quote must not break out of the BBQL string
        // literal — it is backslash-escaped before percent-encoding.
        let f = PrFilter {
            state: "OPEN".to_owned(),
            base: Some(r#"main" OR "1"="1"#.to_owned()),
            limit: 10,
        };
        let path = f.path(&repo());
        let expected_q =
            percent_encode(r#"state="OPEN" AND destination.branch.name="main\" OR \"1\"=\"1""#);
        assert!(path.contains(&format!("&q={expected_q}")), "path: {path}");
    }

    #[test]
    fn list_paginates_the_filter_path() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list",
            FakeTransport::rest(Method::Get, "/pullrequests"),
            FakeTransport::json(200, r#"{"values":[{"id":7,"title":"T","state":"OPEN"}]}"#),
        );
        let client = BitbucketClient::new(h.clone(), None);
        let f = PrFilter {
            state: "OPEN".to_owned(),
            base: None,
            limit: 5,
        };
        let prs = list(&client, &repo(), &f).unwrap();
        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0].id, 7);
        assert!(h.requests.lock().unwrap()[0].url.contains("pagelen=5"));
    }

    #[test]
    fn checks_hits_commit_statuses_path() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "statuses",
            FakeTransport::rest(Method::Get, "/commit/abc123/statuses"),
            FakeTransport::json(200, r#"{"values":[{"key":"build","state":"SUCCESSFUL"}]}"#),
        );
        let client = BitbucketClient::new(h, None);
        let statuses = checks(&client, &repo(), "abc123").unwrap();
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].state.as_deref(), Some("SUCCESSFUL"));
    }
}
