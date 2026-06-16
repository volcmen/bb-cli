//! In-process HTTP test harness — the analog of `gh`'s `httpmock` registry.
//!
//! A [`FakeTransport`] is a [`Transport`] that matches requests against
//! registered stubs and returns canned responses. On drop it asserts that every
//! registered stub was matched (the analog of `gh`'s `Verify(t)`), so a test
//! that forgets to exercise a stubbed call fails loudly.

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::core::{ApiError, HttpRequest, HttpResponse, Method, Transport};

/// Decides whether a stub applies to a request.
pub type Matcher = Box<dyn Fn(&HttpRequest) -> bool + Send + Sync>;
/// Produces a response for a matched request.
pub type Responder = Box<dyn Fn(&HttpRequest) -> HttpResponse + Send + Sync>;

struct Stub {
    label: String,
    matcher: Matcher,
    responder: Responder,
    matched: bool,
}

/// An in-process fake transport for hermetic tests.
#[derive(Default)]
pub struct FakeTransport {
    stubs: Mutex<Vec<Stub>>,
    /// Every request executed, in order — assert on payloads after the run.
    pub requests: Mutex<Vec<HttpRequest>>,
}

impl FakeTransport {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a stub. `label` is shown if the stub is never matched.
    pub fn stub(&self, label: impl Into<String>, matcher: Matcher, responder: Responder) {
        self.stubs.lock().expect("stubs poisoned").push(Stub {
            label: label.into(),
            matcher,
            responder,
            matched: false,
        });
    }

    /// Matcher: method equals `method` and the request URL contains `path`.
    #[must_use]
    pub fn rest(method: Method, path: &str) -> Matcher {
        let path = path.to_owned();
        Box::new(move |req| req.method == method && req.url.contains(&path))
    }

    /// Responder: a JSON body with the given status.
    #[must_use]
    pub fn json(status: u16, body: &str) -> Responder {
        let body = body.as_bytes().to_vec();
        Box::new(move |_req| HttpResponse {
            status,
            headers: BTreeMap::new(),
            body: body.clone(),
        })
    }

    /// Responder: a plain-text body with the given status (for diff/patch).
    #[must_use]
    pub fn text(status: u16, body: &str) -> Responder {
        let body = body.as_bytes().to_vec();
        Box::new(move |_req| HttpResponse {
            status,
            headers: BTreeMap::new(),
            body: body.clone(),
        })
    }

    /// The number of requests executed so far.
    #[must_use]
    pub fn request_count(&self) -> usize {
        self.requests.lock().expect("requests poisoned").len()
    }
}

impl Transport for FakeTransport {
    fn execute(&self, req: HttpRequest) -> Result<HttpResponse, ApiError> {
        self.requests
            .lock()
            .expect("requests poisoned")
            .push(req.clone());
        let mut stubs = self.stubs.lock().expect("stubs poisoned");
        for stub in stubs.iter_mut() {
            if !stub.matched && (stub.matcher)(&req) {
                stub.matched = true;
                return Ok((stub.responder)(&req));
            }
        }
        panic!(
            "FakeTransport: no stub matched {} {}",
            req.method.as_str(),
            req.url
        );
    }
}

impl Drop for FakeTransport {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        let stubs = self.stubs.lock().expect("stubs poisoned");
        let unmatched: Vec<&str> = stubs
            .iter()
            .filter(|s| !s.matched)
            .map(|s| s.label.as_str())
            .collect();
        assert!(
            unmatched.is_empty(),
            "FakeTransport: these stubs were never matched: {unmatched:?}"
        );
    }
}
