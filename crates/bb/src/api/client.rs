//! [`BitbucketClient`] — typed REST calls + body-based pagination over the
//! [`Transport`] seam.

use std::sync::Arc;

use crate::core::{ApiError, ApiErrorItem, HttpRequest, HttpResponse, Method, Transport};
use serde::de::DeserializeOwned;
use serde::Deserialize;

/// The default Bitbucket Cloud API base URL.
pub const CLOUD_BASE_URL: &str = "https://api.bitbucket.org/2.0";

/// One page of a paginated Bitbucket collection.
#[derive(Debug, Deserialize)]
pub struct Page<T> {
    #[serde(default = "Vec::new")]
    pub values: Vec<T>,
    pub next: Option<String>,
    pub page: Option<u64>,
    pub pagelen: Option<u64>,
    pub size: Option<u64>,
}

/// A typed client for the Bitbucket REST API.
pub struct BitbucketClient {
    transport: Arc<dyn Transport>,
    base_url: String,
    /// Full `Authorization` header value (e.g. `Basic ...` or `Bearer ...`),
    /// computed by the auth layer. `None` for unauthenticated calls/tests.
    auth_header: Option<String>,
}

impl BitbucketClient {
    /// A client targeting Bitbucket Cloud.
    #[must_use]
    pub fn new(transport: Arc<dyn Transport>, auth_header: Option<String>) -> Self {
        Self::with_base_url(transport, auth_header, CLOUD_BASE_URL)
    }

    /// A client targeting an explicit base URL (Data Center / tests).
    #[must_use]
    pub fn with_base_url(
        transport: Arc<dyn Transport>,
        auth_header: Option<String>,
        base_url: impl Into<String>,
    ) -> Self {
        Self {
            transport,
            base_url: base_url.into(),
            auth_header,
        }
    }

    fn full_url(&self, path: &str) -> String {
        if path.starts_with("http://") || path.starts_with("https://") {
            path.to_owned()
        } else {
            // Tolerate a missing (or extra) leading slash on `path`: join base and
            // path with exactly one `/` between them, so `user` and `/user` both
            // resolve to `.../2.0/user`.
            format!(
                "{}/{}",
                self.base_url.trim_end_matches('/'),
                path.trim_start_matches('/')
            )
        }
    }

    fn build_request(&self, method: Method, url: &str, body: Option<Vec<u8>>) -> HttpRequest {
        let mut req = HttpRequest::new(method, url).header("Accept", "application/json");
        if let Some(h) = &self.auth_header {
            req = req.header("Authorization", h.clone());
        }
        if let Some(b) = body {
            req = req.header("Content-Type", "application/json").body(b);
        }
        req
    }

    fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<Vec<u8>>,
    ) -> Result<HttpResponse, ApiError> {
        let url = self.full_url(path);
        let resp = self
            .transport
            .execute(self.build_request(method, &url, body))?;
        if resp.is_success() {
            Ok(resp)
        } else {
            Err(map_http_error(&url, &resp))
        }
    }

    /// Send an arbitrary request and return the raw response **without** treating
    /// a non-2xx status as an error (for the `bb api` passthrough). The auth
    /// header and base URL are still applied; only transport failures error.
    ///
    /// # Errors
    /// Returns [`ApiError::Network`] on transport failure.
    pub fn execute_raw(
        &self,
        method: Method,
        path: &str,
        body: Option<Vec<u8>>,
    ) -> Result<HttpResponse, ApiError> {
        let url = self.full_url(path);
        self.transport
            .execute(self.build_request(method, &url, body))
    }

    /// `GET path` → deserialize the JSON body as `T`.
    ///
    /// # Errors
    /// Returns [`ApiError`] on transport failure, non-2xx status, or decode
    /// failure.
    pub fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, ApiError> {
        let resp = self.send(Method::Get, path, None)?;
        decode(&resp.body)
    }

    /// `POST path` with a JSON `body` → deserialize the JSON response as `T`.
    ///
    /// # Errors
    /// As [`BitbucketClient::get`], plus serialization failure of `body`.
    pub fn post<T: DeserializeOwned, B: serde::Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ApiError> {
        let bytes = serde_json::to_vec(body).map_err(|e| ApiError::Decode(e.to_string()))?;
        let resp = self.send(Method::Post, path, Some(bytes))?;
        decode(&resp.body)
    }

    /// `PUT path` with a JSON `body` → deserialize the JSON response as `T`.
    ///
    /// # Errors
    /// As [`BitbucketClient::get`], plus serialization failure of `body`.
    pub fn put<T: DeserializeOwned, B: serde::Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ApiError> {
        let bytes = serde_json::to_vec(body).map_err(|e| ApiError::Decode(e.to_string()))?;
        let resp = self.send(Method::Put, path, Some(bytes))?;
        decode(&resp.body)
    }

    /// Send a `multipart/form-data` request (e.g. Snippet create/edit) and
    /// deserialize the JSON response as `T`. Built separately from
    /// [`BitbucketClient::send`] because [`BitbucketClient::build_request`] always
    /// sets a JSON `Content-Type` when a body is present.
    ///
    /// # Errors
    /// Returns [`ApiError`] on transport failure, non-2xx status, or decode
    /// failure.
    pub fn send_multipart<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        parts: &[MultipartPart],
    ) -> Result<T, ApiError> {
        let (body, content_type) = encode_multipart(parts);
        let url = self.full_url(path);
        let mut req = HttpRequest::new(method, &url).header("Accept", "application/json");
        if let Some(h) = &self.auth_header {
            req = req.header("Authorization", h.clone());
        }
        req = req.header("Content-Type", content_type).body(body);

        let resp = self.transport.execute(req)?;
        if resp.is_success() {
            decode(&resp.body)
        } else {
            Err(map_http_error(&url, &resp))
        }
    }

    /// Send a request whose response body we don't parse (e.g. approve/decline).
    ///
    /// # Errors
    /// Returns [`ApiError`] on transport failure or non-2xx status.
    pub fn send_empty(&self, method: Method, path: &str) -> Result<(), ApiError> {
        self.send(method, path, None).map(|_| ())
    }

    /// `GET path` returning the raw response body as text (e.g. PR diff).
    ///
    /// # Errors
    /// Returns [`ApiError`] on transport failure or non-2xx status.
    pub fn get_raw(&self, path: &str) -> Result<String, ApiError> {
        let resp = self.send(Method::Get, path, None)?;
        Ok(resp.body_str().into_owned())
    }

    /// Walk a paginated collection, following the body `next` URL, collecting up
    /// to `limit` items (or all if `None`).
    ///
    /// # Errors
    /// Returns [`ApiError`] on transport failure, non-2xx status, or decode
    /// failure on any page.
    pub fn paginate<T: DeserializeOwned>(
        &self,
        path: &str,
        limit: Option<usize>,
    ) -> Result<Vec<T>, ApiError> {
        let mut out: Vec<T> = Vec::new();
        if limit == Some(0) {
            return Ok(out);
        }
        let mut url = self.full_url(path);
        loop {
            let resp = self
                .transport
                .execute(self.build_request(Method::Get, &url, None))?;
            if !resp.is_success() {
                return Err(map_http_error(&url, &resp));
            }
            let page: Page<T> = decode(&resp.body)?;
            for value in page.values {
                out.push(value);
                if let Some(l) = limit {
                    if out.len() >= l {
                        return Ok(out);
                    }
                }
            }
            match page.next {
                Some(next) => url = next,
                None => break,
            }
        }
        Ok(out)
    }
}

fn decode<T: DeserializeOwned>(body: &[u8]) -> Result<T, ApiError> {
    serde_json::from_slice(body).map_err(|e| ApiError::Decode(e.to_string()))
}

/// One part of a `multipart/form-data` body. A `filename` makes it a file part
/// (Bitbucket uses the form field name as the snippet filename).
pub struct MultipartPart {
    pub field_name: String,
    pub filename: Option<String>,
    pub value: Vec<u8>,
}

impl MultipartPart {
    /// A file part (field name = filename, per Bitbucket's snippet API).
    #[must_use]
    pub fn file(filename: impl Into<String>, value: Vec<u8>) -> Self {
        let filename = filename.into();
        Self {
            field_name: filename.clone(),
            filename: Some(filename),
            value,
        }
    }

    /// A plain text field (e.g. `title`, `is_private`).
    #[must_use]
    pub fn field(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            field_name: name.into(),
            filename: None,
            value: value.into().into_bytes(),
        }
    }
}

/// A fixed boundary — extremely unlikely to collide with snippet content.
const MULTIPART_BOUNDARY: &str = "bbCLIb0undaryQx7vK3mZ9pLw2";

/// Escape a value before it goes inside a quoted `Content-Disposition` parameter.
/// Per RFC 7578 §5.1 we percent-encode the characters that would otherwise break
/// out of the quoted string or inject extra headers: `"`, CR and LF. Without this
/// a field name or filename like `a"\r\nX: y` could forge header lines.
fn escape_disposition(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('"', "%22")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

/// Encode `parts` into a `multipart/form-data` body, returning the body bytes and
/// the matching `Content-Type` value.
fn encode_multipart(parts: &[MultipartPart]) -> (Vec<u8>, String) {
    let mut body = Vec::new();
    for part in parts {
        body.extend_from_slice(format!("--{MULTIPART_BOUNDARY}\r\n").as_bytes());
        let name = escape_disposition(&part.field_name);
        match &part.filename {
            Some(filename) => body.extend_from_slice(
                format!(
                    "Content-Disposition: form-data; name=\"{name}\"; filename=\"{}\"\r\n\r\n",
                    escape_disposition(filename)
                )
                .as_bytes(),
            ),
            None => body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
            ),
        }
        body.extend_from_slice(&part.value);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{MULTIPART_BOUNDARY}--\r\n").as_bytes());

    let content_type = format!("multipart/form-data; boundary={MULTIPART_BOUNDARY}");
    (body, content_type)
}

fn map_http_error(url: &str, resp: &HttpResponse) -> ApiError {
    let (message, errors) = parse_error_body(&resp.body).unwrap_or_else(|| {
        (
            format!("request failed with status {}", resp.status),
            Vec::new(),
        )
    });
    ApiError::Http {
        status: resp.status,
        url: url.to_owned(),
        message,
        errors,
    }
}

/// Best-effort parse of Bitbucket's `{ "error": { "message": ..., "fields": {...} } }`.
fn parse_error_body(body: &[u8]) -> Option<(String, Vec<ApiErrorItem>)> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    let err = v.get("error")?;
    let message = err.get("message")?.as_str()?.to_owned();
    let mut items = Vec::new();
    if let Some(fields) = err.get("fields").and_then(|f| f.as_object()) {
        for (field, msgs) in fields {
            let msg = msgs
                .as_array()
                .and_then(|a| a.first())
                .and_then(|m| m.as_str())
                .unwrap_or("invalid")
                .to_owned();
            items.push(ApiErrorItem {
                field: Some(field.clone()),
                message: msg,
            });
        }
    }
    Some((message, items))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::{PullRequest, User};
    use crate::api::testing::FakeTransport;

    #[test]
    fn multipart_escapes_quotes_and_crlf_in_disposition() {
        // A filename containing a quote + CRLF must not break out of the quoted
        // Content-Disposition value or forge extra header lines.
        let parts = vec![MultipartPart::file(
            "evil\"\r\nX-Inject: y.txt",
            b"data".to_vec(),
        )];
        let (body, _ct) = encode_multipart(&parts);
        let text = String::from_utf8_lossy(&body);
        let header = text
            .lines()
            .find(|l| l.starts_with("Content-Disposition"))
            .unwrap();
        // No raw quote/CR/LF leaked into the header; they are percent-encoded.
        assert!(
            header.contains("filename=\"evil%22%0D%0AX-Inject: y.txt\""),
            "{header}"
        );
        assert!(
            !text.contains("\r\nX-Inject: y.txt"),
            "CRLF injected: {text}"
        );
    }

    #[test]
    fn get_parses_typed_response() {
        let fake = Arc::new(FakeTransport::new());
        fake.stub(
            "GET /user",
            FakeTransport::rest(Method::Get, "/2.0/user"),
            FakeTransport::json(200, r#"{"username":"davidd","display_name":"David D"}"#),
        );
        let client = BitbucketClient::new(fake, None);
        let user: User = client.get("/user").unwrap();
        assert_eq!(user.username.as_deref(), Some("davidd"));
    }

    #[test]
    fn path_with_or_without_leading_slash_resolves_the_same() {
        let fake = Arc::new(FakeTransport::new());
        // Two stubs, one per call: the matcher requires `/2.0/user`, so a missing
        // leading slash that produced `/2.0user` would fail to match → panic.
        fake.stub(
            "GET /user (no slash)",
            FakeTransport::rest(Method::Get, "/2.0/user"),
            FakeTransport::json(200, r#"{"username":"davidd"}"#),
        );
        fake.stub(
            "GET /user (leading slash)",
            FakeTransport::rest(Method::Get, "/2.0/user"),
            FakeTransport::json(200, r#"{"username":"davidd"}"#),
        );
        let client = BitbucketClient::new(fake.clone(), None);

        let no_slash: User = client.get("user").unwrap();
        let leading_slash: User = client.get("/user").unwrap();
        assert_eq!(no_slash.username.as_deref(), Some("davidd"));
        assert_eq!(leading_slash.username.as_deref(), Some("davidd"));

        let reqs = fake.requests.lock().unwrap();
        assert!(reqs[0].url.contains("/2.0/user"), "url: {}", reqs[0].url);
        assert!(reqs[1].url.contains("/2.0/user"), "url: {}", reqs[1].url);
    }

    #[test]
    fn non_2xx_maps_to_http_error() {
        let fake = Arc::new(FakeTransport::new());
        fake.stub(
            "GET /user 401",
            FakeTransport::rest(Method::Get, "/2.0/user"),
            FakeTransport::json(401, r#"{"type":"error","error":{"message":"bad creds"}}"#),
        );
        let client = BitbucketClient::new(fake, None);
        let err = client.get::<User>("/user").unwrap_err();
        assert!(err.is_unauthorized());
        assert_eq!(err.status(), Some(401));
    }

    // ---- headers ----

    #[test]
    fn get_sends_accept_and_authorization_headers() {
        let fake = Arc::new(FakeTransport::new());
        fake.stub(
            "GET /user",
            FakeTransport::rest(Method::Get, "/2.0/user"),
            FakeTransport::json(200, r#"{"username":"davidd"}"#),
        );
        let client = BitbucketClient::new(fake.clone(), Some("Bearer tok".to_owned()));
        let _: User = client.get("/user").unwrap();

        let reqs = fake.requests.lock().unwrap();
        let req = &reqs[0];
        assert_eq!(
            req.headers.get("Accept").map(String::as_str),
            Some("application/json")
        );
        assert_eq!(
            req.headers.get("Authorization").map(String::as_str),
            Some("Bearer tok")
        );
        // GET has no body, so no Content-Type.
        assert!(!req.headers.contains_key("Content-Type"));
        assert!(req.body.is_none());
    }

    #[test]
    fn unauthenticated_client_omits_authorization() {
        let fake = Arc::new(FakeTransport::new());
        fake.stub(
            "GET /user",
            FakeTransport::rest(Method::Get, "/2.0/user"),
            FakeTransport::json(200, r#"{"username":"davidd"}"#),
        );
        let client = BitbucketClient::new(fake.clone(), None);
        let _: User = client.get("/user").unwrap();
        let reqs = fake.requests.lock().unwrap();
        assert!(!reqs[0].headers.contains_key("Authorization"));
    }

    // ---- post: serializes body, sets Content-Type, parses response ----

    #[test]
    fn post_serializes_json_body_and_parses_response() {
        let fake = Arc::new(FakeTransport::new());
        fake.stub(
            "POST /repositories/.../pullrequests",
            FakeTransport::rest(Method::Post, "/pullrequests"),
            FakeTransport::json(201, r#"{"id":7,"title":"Add widget","state":"OPEN"}"#),
        );
        let client = BitbucketClient::new(fake.clone(), Some("Bearer t".to_owned()));

        let body = serde_json::json!({
            "title": "Add widget",
            "source": { "branch": { "name": "feature/x" } },
        });
        let pr: PullRequest = client
            .post("/repositories/acme/widgets/pullrequests", &body)
            .unwrap();
        assert_eq!(pr.id, 7);
        assert_eq!(pr.title.as_deref(), Some("Add widget"));

        let reqs = fake.requests.lock().unwrap();
        let req = &reqs[0];
        assert_eq!(req.method, Method::Post);
        assert_eq!(
            req.headers.get("Content-Type").map(String::as_str),
            Some("application/json")
        );
        // The body must be valid JSON carrying our fields.
        let sent: serde_json::Value =
            serde_json::from_slice(req.body.as_ref().expect("body present")).unwrap();
        assert_eq!(sent["title"], "Add widget");
        assert_eq!(sent["source"]["branch"]["name"], "feature/x");
    }

    // ---- get_raw: returns the text body verbatim ----

    #[test]
    fn get_raw_returns_text_body() {
        let diff = "diff --git a/x b/x\n@@ -1 +1 @@\n-old\n+new\n";
        let fake = Arc::new(FakeTransport::new());
        fake.stub(
            "GET diff",
            FakeTransport::rest(Method::Get, "/diff"),
            FakeTransport::text(200, diff),
        );
        let client = BitbucketClient::new(fake, None);
        let got = client.get_raw("/repositories/acme/widgets/diff/1").unwrap();
        assert_eq!(got, diff);
    }

    // ---- pagination ----

    #[test]
    fn paginate_follows_next_across_pages() {
        let fake = Arc::new(FakeTransport::new());
        // Page 1 points at an absolute `next` URL; client must follow it verbatim.
        fake.stub(
            "page 1",
            FakeTransport::rest(Method::Get, "/2.0/repositories/acme/widgets/pullrequests"),
            FakeTransport::json(
                200,
                r#"{"values":[{"id":1},{"id":2}],"next":"https://api.bitbucket.org/2.0/repositories/acme/widgets/pullrequests?page=2"}"#,
            ),
        );
        fake.stub(
            "page 2",
            FakeTransport::rest(Method::Get, "pullrequests?page=2"),
            FakeTransport::json(200, r#"{"values":[{"id":3}]}"#),
        );
        let client = BitbucketClient::new(fake.clone(), None);
        let prs: Vec<PullRequest> = client
            .paginate("/repositories/acme/widgets/pullrequests", None)
            .unwrap();
        let ids: Vec<u64> = prs.iter().map(|p| p.id).collect();
        assert_eq!(ids, vec![1, 2, 3]);
        assert_eq!(fake.request_count(), 2);
    }

    #[test]
    fn paginate_honors_limit_and_stops_early() {
        let fake = Arc::new(FakeTransport::new());
        // Only page 1 is stubbed. If the client fetched page 2, the unmatched
        // first-page `next` would force a request to an unstubbed URL → panic.
        fake.stub(
            "page 1",
            FakeTransport::rest(Method::Get, "/2.0/repositories/acme/widgets/pullrequests"),
            FakeTransport::json(
                200,
                r#"{"values":[{"id":1},{"id":2}],"next":"https://api.bitbucket.org/2.0/x?page=2"}"#,
            ),
        );
        let client = BitbucketClient::new(fake.clone(), None);
        let prs: Vec<PullRequest> = client
            .paginate("/repositories/acme/widgets/pullrequests", Some(2))
            .unwrap();
        assert_eq!(prs.len(), 2);
        // Must have stopped after the first page (no second request).
        assert_eq!(fake.request_count(), 1);
    }

    #[test]
    fn paginate_limit_smaller_than_first_page() {
        let fake = Arc::new(FakeTransport::new());
        fake.stub(
            "page 1",
            FakeTransport::rest(Method::Get, "/2.0/items"),
            FakeTransport::json(
                200,
                r#"{"values":[{"id":1},{"id":2},{"id":3}],"next":"https://api.bitbucket.org/2.0/items?page=2"}"#,
            ),
        );
        let client = BitbucketClient::new(fake.clone(), None);
        let prs: Vec<PullRequest> = client.paginate("/items", Some(1)).unwrap();
        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0].id, 1);
        assert_eq!(fake.request_count(), 1);
    }

    #[test]
    fn paginate_empty_collection() {
        let fake = Arc::new(FakeTransport::new());
        fake.stub(
            "empty",
            FakeTransport::rest(Method::Get, "/2.0/items"),
            FakeTransport::json(200, r#"{"values":[]}"#),
        );
        let client = BitbucketClient::new(fake, None);
        let prs: Vec<PullRequest> = client.paginate("/items", None).unwrap();
        assert!(prs.is_empty());
    }

    // ---- error mapping across statuses + parsed messages ----

    fn error_body(msg: &str) -> String {
        format!(r#"{{"type":"error","error":{{"message":"{msg}"}}}}"#)
    }

    #[test]
    fn maps_403_with_parsed_message() {
        let fake = Arc::new(FakeTransport::new());
        fake.stub(
            "403",
            FakeTransport::rest(Method::Get, "/2.0/forbidden"),
            FakeTransport::json(403, &error_body("forbidden")),
        );
        let client = BitbucketClient::new(fake, None);
        let err = client.get::<User>("/forbidden").unwrap_err();
        assert_eq!(err.status(), Some(403));
        assert!(!err.is_unauthorized());
        assert!(!err.is_not_found());
        match &err {
            ApiError::Http { message, .. } => assert_eq!(message, "forbidden"),
            other => panic!("expected Http error, got {other:?}"),
        }
    }

    #[test]
    fn maps_404_with_parsed_message_and_is_not_found() {
        let fake = Arc::new(FakeTransport::new());
        fake.stub(
            "404",
            FakeTransport::rest(Method::Get, "/2.0/missing"),
            FakeTransport::json(404, &error_body("no such repository")),
        );
        let client = BitbucketClient::new(fake, None);
        let err = client.get::<User>("/missing").unwrap_err();
        assert_eq!(err.status(), Some(404));
        assert!(err.is_not_found());
        match &err {
            ApiError::Http { message, .. } => assert_eq!(message, "no such repository"),
            other => panic!("expected Http error, got {other:?}"),
        }
    }

    #[test]
    fn maps_429_with_parsed_message() {
        let fake = Arc::new(FakeTransport::new());
        fake.stub(
            "429",
            FakeTransport::rest(Method::Get, "/2.0/limited"),
            FakeTransport::json(429, &error_body("rate limited")),
        );
        let client = BitbucketClient::new(fake, None);
        let err = client.get::<User>("/limited").unwrap_err();
        assert_eq!(err.status(), Some(429));
        match &err {
            ApiError::Http { message, .. } => assert_eq!(message, "rate limited"),
            other => panic!("expected Http error, got {other:?}"),
        }
    }

    #[test]
    fn maps_500_with_parsed_message() {
        let fake = Arc::new(FakeTransport::new());
        fake.stub(
            "500",
            FakeTransport::rest(Method::Get, "/2.0/boom"),
            FakeTransport::json(500, &error_body("internal error")),
        );
        let client = BitbucketClient::new(fake, None);
        let err = client.get::<User>("/boom").unwrap_err();
        assert_eq!(err.status(), Some(500));
        match &err {
            ApiError::Http { message, url, .. } => {
                assert_eq!(message, "internal error");
                assert!(url.contains("/boom"));
            }
            other => panic!("expected Http error, got {other:?}"),
        }
    }

    #[test]
    fn error_without_envelope_falls_back_to_generic_message() {
        let fake = Arc::new(FakeTransport::new());
        fake.stub(
            "503 no envelope",
            FakeTransport::rest(Method::Get, "/2.0/down"),
            // No `error` object — e.g. an HTML/plain body or empty.
            FakeTransport::text(503, "Service Unavailable"),
        );
        let client = BitbucketClient::new(fake, None);
        let err = client.get::<User>("/down").unwrap_err();
        assert_eq!(err.status(), Some(503));
        match &err {
            ApiError::Http {
                message, errors, ..
            } => {
                assert_eq!(message, "request failed with status 503");
                assert!(errors.is_empty());
            }
            other => panic!("expected Http error, got {other:?}"),
        }
    }

    #[test]
    fn paginate_surfaces_http_error_on_a_page() {
        let fake = Arc::new(FakeTransport::new());
        fake.stub(
            "page error",
            FakeTransport::rest(Method::Get, "/2.0/items"),
            FakeTransport::json(500, &error_body("boom")),
        );
        let client = BitbucketClient::new(fake, None);
        let err = client.paginate::<PullRequest>("/items", None).unwrap_err();
        assert_eq!(err.status(), Some(500));
    }
}
