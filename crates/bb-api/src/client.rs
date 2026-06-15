//! [`BitbucketClient`] — typed REST calls + body-based pagination over the
//! [`Transport`] seam.

use std::sync::Arc;

use bb_core::{ApiError, ApiErrorItem, HttpRequest, HttpResponse, Method, Transport};
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
            format!("{}{}", self.base_url, path)
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
    use crate::models::User;
    use crate::testing::FakeTransport;

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
}
