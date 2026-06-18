//! Render-free issue *fetch* helpers shared by the CLI commands and the TUI
//! (spec 033, extended for the Issues section #86). Own path-building + pagination.

use crate::api::models::Issue;
use crate::api::BitbucketClient;
use crate::core::{ApiError, RepoId};
use crate::render::percent_encode;

/// What issues to list — built from `issue list` flags or a TUI section.
#[derive(Debug, Clone)]
pub struct IssueFilter {
    pub state: Option<String>,
    pub limit: usize,
}

impl IssueFilter {
    /// The exact `issues` query path (pagelen clamped to 1..=50; an optional state
    /// goes through a BBQL `q`, matching `bb issue list`).
    pub(crate) fn path(&self, repo: &RepoId) -> String {
        let pagelen = self.limit.clamp(1, 50);
        let mut path = format!(
            "/repositories/{}/{}/issues?sort=-updated_on&pagelen={pagelen}",
            repo.workspace(),
            repo.slug(),
        );
        if let Some(state) = &self.state {
            path.push_str(&format!(
                "&q={}",
                percent_encode(&format!("state=\"{state}\""))
            ));
        }
        path
    }
}

/// List issues matching `filter`.
///
/// # Errors
/// Propagates [`ApiError`] (a disabled tracker surfaces as 404/410 — callers map
/// it via [`ApiError::is_gone`]/[`ApiError::is_not_found`]).
pub fn list(
    client: &BitbucketClient,
    repo: &RepoId,
    filter: &IssueFilter,
) -> Result<Vec<Issue>, ApiError> {
    client.paginate(&filter.path(repo), Some(filter.limit))
}

/// Fetch a single issue by id.
///
/// # Errors
/// Propagates [`ApiError`].
pub fn get(client: &BitbucketClient, repo: &RepoId, id: u64) -> Result<Issue, ApiError> {
    let path = format!(
        "/repositories/{}/{}/issues/{id}",
        repo.workspace(),
        repo.slug(),
    );
    client.get(&path)
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
    fn filter_path_default_and_state() {
        let f = IssueFilter {
            state: None,
            limit: 30,
        };
        assert_eq!(
            f.path(&repo()),
            "/repositories/acme/widgets/issues?sort=-updated_on&pagelen=30"
        );
        let f = IssueFilter {
            state: Some("open".to_owned()),
            limit: 100,
        };
        let path = f.path(&repo());
        assert!(path.contains("pagelen=50"), "path: {path}");
        assert!(
            path.contains(&format!("&q={}", percent_encode(r#"state="open""#))),
            "path: {path}"
        );
    }

    #[test]
    fn list_and_get_hit_expected_paths() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list",
            FakeTransport::rest(Method::Get, "/issues?sort"),
            FakeTransport::json(200, r#"{"values":[{"id":3,"title":"Bug"}]}"#),
        );
        h.stub(
            "get",
            FakeTransport::rest(Method::Get, "/issues/3"),
            FakeTransport::json(200, r#"{"id":3,"title":"Bug","state":"new"}"#),
        );
        let client = BitbucketClient::new(h, None);
        let issues = list(
            &client,
            &repo(),
            &IssueFilter {
                state: None,
                limit: 30,
            },
        )
        .unwrap();
        assert_eq!(issues.len(), 1);
        let issue = get(&client, &repo(), 3).unwrap();
        assert_eq!(issue.id, 3);
    }
}
