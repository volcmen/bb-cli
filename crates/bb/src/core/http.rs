//! Transport-agnostic HTTP request/response types used by the [`Transport`]
//! seam.
//!
//! [`Transport`]: crate::core::traits::Transport

use std::borrow::Cow;
use std::collections::BTreeMap;

/// HTTP method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
    Delete,
    Patch,
}

impl Method {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Delete => "DELETE",
            Method::Patch => "PATCH",
        }
    }
}

/// An outbound HTTP request, independent of any concrete client.
#[derive(Clone)]
pub struct HttpRequest {
    pub method: Method,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub body: Option<Vec<u8>>,
}

// Manual Debug so a stray `{:?}`/log can never leak the `Authorization` header
// (bearer token or Basic credentials) or a request body.
impl std::fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let headers: BTreeMap<&str, &str> = self
            .headers
            .iter()
            .map(|(k, v)| {
                let value = if k.eq_ignore_ascii_case("authorization") {
                    "<redacted>"
                } else {
                    v.as_str()
                };
                (k.as_str(), value)
            })
            .collect();
        f.debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("headers", &headers)
            .field(
                "body",
                &self.body.as_ref().map(|b| format!("<{} bytes>", b.len())),
            )
            .finish()
    }
}

impl HttpRequest {
    pub fn new(method: Method, url: impl Into<String>) -> Self {
        Self {
            method,
            url: url.into(),
            headers: BTreeMap::new(),
            body: None,
        }
    }

    /// Add a header (builder style).
    #[must_use]
    pub fn header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }

    /// Attach a raw body (builder style).
    #[must_use]
    pub fn body(mut self, body: Vec<u8>) -> Self {
        self.body = Some(body);
        self
    }
}

/// An inbound HTTP response.
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    #[must_use]
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// The body decoded as UTF-8 (lossy).
    #[must_use]
    pub fn body_str(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_authorization_and_body() {
        let req = HttpRequest::new(Method::Post, "https://api.bitbucket.org/2.0/user")
            .header("Authorization", "Bearer super-secret-token")
            .header("Accept", "application/json")
            .body(b"grant_type=authorization_code&code=abc".to_vec());
        let shown = format!("{req:?}");
        assert!(
            !shown.contains("super-secret-token"),
            "token leaked: {shown}"
        );
        assert!(shown.contains("<redacted>"), "missing redaction: {shown}");
        // Non-secret headers still print; the body is summarized, not dumped.
        assert!(shown.contains("application/json"));
        assert!(!shown.contains("grant_type"), "body leaked: {shown}");
        assert!(
            shown.contains("bytes>"),
            "body should be byte-counted: {shown}"
        );
    }
}
