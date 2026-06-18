//! The background data worker (DDR 0003: async-without-tokio).
//!
//! A `std::thread` owns a [`BitbucketClient`] built over the (refresh-on-401)
//! transport and the service layer (spec 033). The UI sends [`Request`]s and
//! receives [`Response`]s over `std::sync::mpsc` channels, so the 60fps loop never
//! blocks on the blocking transport. A `401` is handled transparently by the
//! transport's refresh decorator; a failed refresh surfaces as [`Response::Error`].

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;

use crate::api::models::{CommitStatus, PullRequest};
use crate::api::BitbucketClient;
use crate::commands::pr::query::{self, PrFilter};
use crate::core::{RepoId, Transport};
use std::sync::Arc;

/// Tags a response back to the request that produced it (drives the spinner and
/// lets the reducer clear the right in-flight flag).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestKind {
    Prs,
    PrDetail,
}

/// A unit of work for the worker thread.
#[derive(Debug, Clone)]
pub enum Request {
    /// Fetch the pull-request list for a filter.
    Prs(PrFilter),
    /// Fetch one PR plus its CI checks (the detail pane).
    PrDetail(u64),
}

/// A result delivered back to the UI thread.
#[derive(Debug)]
pub enum Response {
    Prs(Vec<PullRequest>),
    /// A full PR plus its CI checks (fetched together so the pane renders at once).
    PrDetail {
        pr: Box<PullRequest>,
        checks: Vec<CommitStatus>,
    },
    /// A human-facing error message + the request kind it belongs to.
    Error(String, RequestKind),
}

/// Handle to the worker: send [`Request`]s, drain [`Response`]s.
pub struct Worker {
    tx: Option<Sender<Request>>,
    pub rx: Receiver<Response>,
    handle: Option<JoinHandle<()>>,
}

impl Worker {
    /// Spawn a worker bound to `repo`, authenticating with `header`.
    #[must_use]
    pub fn spawn(transport: Arc<dyn Transport>, header: Option<String>, repo: RepoId) -> Self {
        let (req_tx, req_rx) = mpsc::channel::<Request>();
        let (resp_tx, resp_rx) = mpsc::channel::<Response>();

        let handle = std::thread::spawn(move || {
            let client = BitbucketClient::new(transport, header);
            // The loop ends when the UI drops `tx` (channel closed) or a send fails
            // (UI gone) — either way the thread exits cleanly.
            while let Ok(request) = req_rx.recv() {
                let response = handle_request(&client, &repo, request);
                if resp_tx.send(response).is_err() {
                    break;
                }
            }
        });

        Self {
            tx: Some(req_tx),
            rx: resp_rx,
            handle: Some(handle),
        }
    }

    /// Queue a request. Returns `false` if the worker has gone away.
    pub fn send(&self, request: Request) -> bool {
        self.tx.as_ref().is_some_and(|tx| tx.send(request).is_ok())
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // Drop the sender FIRST so the request channel closes and the thread's
        // `recv` returns Err (loop exits) — only then can the join complete
        // without deadlocking on a still-open channel.
        self.tx = None;
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Execute one request against the service layer, mapping errors to a message.
fn handle_request(client: &BitbucketClient, repo: &RepoId, request: Request) -> Response {
    match request {
        Request::Prs(filter) => match query::list(client, repo, &filter) {
            Ok(prs) => Response::Prs(prs),
            Err(e) => Response::Error(format!("{e}"), RequestKind::Prs),
        },
        Request::PrDetail(id) => match query::get(client, repo, id) {
            Ok(pr) => {
                // Fetch checks for the head commit when known; a checks failure is
                // non-fatal (the pane still shows the PR, just no CI rows).
                let checks = pr
                    .source
                    .commit_hash()
                    .map(|sha| query::checks(client, repo, sha).unwrap_or_default())
                    .unwrap_or_default();
                Response::PrDetail {
                    pr: Box::new(pr),
                    checks,
                }
            }
            Err(e) => Response::Error(format!("{e}"), RequestKind::PrDetail),
        },
    }
}

#[cfg(test)]
mod tests {
    use crate::api::testing::FakeTransport;
    use crate::core::Method;

    use super::*;

    fn filter() -> PrFilter {
        PrFilter {
            state: "OPEN".to_owned(),
            base: None,
            limit: 30,
        }
    }

    #[test]
    fn request_prs_yields_response_prs() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list",
            FakeTransport::rest(Method::Get, "/pullrequests"),
            FakeTransport::json(200, r#"{"values":[{"id":7,"title":"T","state":"OPEN"}]}"#),
        );
        let transport: Arc<dyn Transport> = h;
        let worker = Worker::spawn(transport, None, RepoId::new("acme", "widgets"));

        assert!(worker.send(Request::Prs(filter())));
        match worker.rx.recv().unwrap() {
            Response::Prs(prs) => {
                assert_eq!(prs.len(), 1);
                assert_eq!(prs[0].id, 7);
            }
            other => panic!("expected Prs, got {other:?}"),
        }
    }

    #[test]
    fn request_pr_detail_fetches_pr_and_checks() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get pr",
            FakeTransport::rest(Method::Get, "/pullrequests/42"),
            FakeTransport::json(
                200,
                r#"{"id":42,"title":"T","state":"OPEN","source":{"commit":{"hash":"abc"}}}"#,
            ),
        );
        h.stub(
            "checks",
            FakeTransport::rest(Method::Get, "/commit/abc/statuses"),
            FakeTransport::json(200, r#"{"values":[{"key":"build","state":"SUCCESSFUL"}]}"#),
        );
        let transport: Arc<dyn Transport> = h;
        let worker = Worker::spawn(transport, None, RepoId::new("acme", "widgets"));

        worker.send(Request::PrDetail(42));
        match worker.rx.recv().unwrap() {
            Response::PrDetail { pr, checks } => {
                assert_eq!(pr.id, 42);
                assert_eq!(checks.len(), 1);
            }
            other => panic!("expected PrDetail, got {other:?}"),
        }
    }

    #[test]
    fn api_error_yields_response_error() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list 500",
            FakeTransport::rest(Method::Get, "/pullrequests"),
            FakeTransport::json(500, r#"{"error":{"message":"boom"}}"#),
        );
        let transport: Arc<dyn Transport> = h;
        let worker = Worker::spawn(transport, None, RepoId::new("acme", "widgets"));

        worker.send(Request::Prs(filter()));
        match worker.rx.recv().unwrap() {
            Response::Error(msg, kind) => {
                assert_eq!(kind, RequestKind::Prs);
                assert!(msg.contains("boom"), "msg: {msg}");
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }
}
