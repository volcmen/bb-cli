//! Render-free pipeline *fetch* helpers shared by the CLI commands and the TUI
//! (spec 033, extended for the Pipelines section #87).

use crate::api::models::{Pipeline, PipelineStep};
use crate::api::BitbucketClient;
use crate::core::{ApiError, RepoId};

/// List recent pipelines (newest first), up to `limit`.
///
/// # Errors
/// Propagates [`ApiError`].
pub fn list(
    client: &BitbucketClient,
    repo: &RepoId,
    limit: usize,
) -> Result<Vec<Pipeline>, ApiError> {
    let pagelen = limit.clamp(1, 50);
    let path = format!(
        "/repositories/{}/{}/pipelines/?sort=-created_on&pagelen={pagelen}",
        repo.workspace(),
        repo.slug(),
    );
    client.paginate(&path, Some(limit))
}

/// Fetch one pipeline (by build number) plus its steps.
///
/// # Errors
/// Propagates [`ApiError`] (a steps failure is non-fatal — returns an empty list).
pub fn detail(
    client: &BitbucketClient,
    repo: &RepoId,
    build_number: u64,
) -> Result<(Pipeline, Vec<PipelineStep>), ApiError> {
    let base = format!(
        "/repositories/{}/{}/pipelines/{build_number}",
        repo.workspace(),
        repo.slug(),
    );
    let pipeline: Pipeline = client.get(&base)?;
    let steps = client
        .paginate(&format!("{base}/steps/"), None)
        .unwrap_or_default();
    Ok((pipeline, steps))
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
    fn list_hits_pipelines_path() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list",
            FakeTransport::rest(Method::Get, "/pipelines/?sort"),
            FakeTransport::json(
                200,
                r#"{"values":[{"build_number":12,"state":{"name":"COMPLETED","result":{"name":"SUCCESSFUL"}}}]}"#,
            ),
        );
        let client = BitbucketClient::new(h, None);
        let pipelines = list(&client, &repo(), 30).unwrap();
        assert_eq!(pipelines[0].build_number, Some(12));
    }

    #[test]
    fn detail_fetches_pipeline_and_steps() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get",
            FakeTransport::rest(Method::Get, "/pipelines/12"),
            FakeTransport::json(200, r#"{"build_number":12,"state":{"name":"COMPLETED"}}"#),
        );
        h.stub(
            "steps",
            FakeTransport::rest(Method::Get, "/pipelines/12/steps/"),
            FakeTransport::json(
                200,
                r#"{"values":[{"name":"Build","state":{"name":"COMPLETED"}}]}"#,
            ),
        );
        let client = BitbucketClient::new(h, None);
        let (pipeline, steps) = detail(&client, &repo(), 12).unwrap();
        assert_eq!(pipeline.build_number, Some(12));
        assert_eq!(steps.len(), 1);
    }
}
