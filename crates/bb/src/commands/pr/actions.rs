//! Render-free pull-request *mutation* helpers, shared by the CLI commands and
//! (per spec 033) the forthcoming TUI. These own the endpoint paths + request
//! bodies; callers resolve repo/auth and render the outcome.

use crate::api::models::PullRequest;
use crate::api::{BitbucketClient, Links};
use crate::core::{ApiError, Method, RepoId};

fn pr_path(repo: &RepoId, id: u64, suffix: &str) -> String {
    format!(
        "/repositories/{}/{}/pullrequests/{id}{suffix}",
        repo.workspace(),
        repo.slug(),
    )
}

/// Approve a pull request.
///
/// # Errors
/// Propagates [`ApiError`].
pub fn approve(client: &BitbucketClient, repo: &RepoId, id: u64) -> Result<(), ApiError> {
    client.send_empty(Method::Post, &pr_path(repo, id, "/approve"))
}

/// Remove your approval from a pull request.
///
/// # Errors
/// Propagates [`ApiError`].
pub fn unapprove(client: &BitbucketClient, repo: &RepoId, id: u64) -> Result<(), ApiError> {
    client.send_empty(Method::Delete, &pr_path(repo, id, "/approve"))
}

#[derive(serde::Serialize)]
struct MergeBody<'a> {
    merge_strategy: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<&'a str>,
    close_source_branch: bool,
}

/// A lenient view of the merge response (Bitbucket may return the merged PR or a
/// 202 async-task envelope with no `state`).
#[derive(serde::Deserialize, Default)]
pub struct MergeOutcome {
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub links: Links,
}

/// Merge a pull request with the given strategy/message/close-source-branch.
///
/// # Errors
/// Propagates [`ApiError`] (e.g. a merge conflict surfaced by Bitbucket).
pub fn merge(
    client: &BitbucketClient,
    repo: &RepoId,
    id: u64,
    strategy: &str,
    message: Option<&str>,
    close_source_branch: bool,
) -> Result<MergeOutcome, ApiError> {
    let body = MergeBody {
        merge_strategy: strategy,
        message,
        close_source_branch,
    };
    client.post(&pr_path(repo, id, "/merge"), &body)
}

#[derive(serde::Serialize)]
struct DeclineBody<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<&'a str>,
}

/// Decline (close) a pull request.
///
/// # Errors
/// Propagates [`ApiError`].
pub fn decline(
    client: &BitbucketClient,
    repo: &RepoId,
    id: u64,
    message: Option<&str>,
) -> Result<PullRequest, ApiError> {
    client.post(&pr_path(repo, id, "/decline"), &DeclineBody { message })
}

#[derive(serde::Serialize)]
struct Content<'a> {
    raw: &'a str,
}

#[derive(serde::Serialize)]
struct CommentBody<'a> {
    content: Content<'a>,
}

/// Post a comment on a pull request, returning the raw created-comment JSON.
///
/// # Errors
/// Propagates [`ApiError`].
pub fn comment(
    client: &BitbucketClient,
    repo: &RepoId,
    id: u64,
    raw: &str,
) -> Result<serde_json::Value, ApiError> {
    let body = CommentBody {
        content: Content { raw },
    };
    client.post(&pr_path(repo, id, "/comments"), &body)
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
    fn approve_posts_to_approve_endpoint() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "approve",
            FakeTransport::rest(Method::Post, "/pullrequests/42/approve"),
            FakeTransport::json(200, r#"{"approved":true}"#),
        );
        let client = BitbucketClient::new(h, None);
        approve(&client, &repo(), 42).unwrap();
    }

    #[test]
    fn unapprove_deletes_approve_endpoint() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "unapprove",
            FakeTransport::rest(Method::Delete, "/pullrequests/42/approve"),
            FakeTransport::json(204, ""),
        );
        let client = BitbucketClient::new(h, None);
        unapprove(&client, &repo(), 42).unwrap();
    }

    #[test]
    fn merge_posts_body() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "merge",
            FakeTransport::rest(Method::Post, "/pullrequests/42/merge"),
            FakeTransport::json(200, r#"{"state":"MERGED"}"#),
        );
        let client = BitbucketClient::new(h.clone(), None);
        let out = merge(&client, &repo(), 42, "squash", Some("msg"), true).unwrap();
        assert_eq!(out.state.as_deref(), Some("MERGED"));
        let reqs = h.requests.lock().unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(reqs[0].body.as_deref().unwrap()).unwrap();
        assert_eq!(body["merge_strategy"], "squash");
        assert_eq!(body["message"], "msg");
        assert_eq!(body["close_source_branch"], true);
    }

    #[test]
    fn decline_posts_to_decline_endpoint() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "decline",
            FakeTransport::rest(Method::Post, "/pullrequests/42/decline"),
            FakeTransport::json(200, r#"{"id":42,"state":"DECLINED"}"#),
        );
        let client = BitbucketClient::new(h, None);
        let pr = decline(&client, &repo(), 42, Some("nope")).unwrap();
        assert_eq!(pr.state.as_deref(), Some("DECLINED"));
    }

    #[test]
    fn comment_posts_raw_content() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "comment",
            FakeTransport::rest(Method::Post, "/pullrequests/42/comments"),
            FakeTransport::json(201, r#"{"id":1}"#),
        );
        let client = BitbucketClient::new(h.clone(), None);
        comment(&client, &repo(), 42, "looks good").unwrap();
        let reqs = h.requests.lock().unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(reqs[0].body.as_deref().unwrap()).unwrap();
        assert_eq!(body["content"]["raw"], "looks good");
    }
}
