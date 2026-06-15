//! The real [`Transport`] implementation, backed by `reqwest`'s blocking client
//! (rustls TLS). Everything network-facing goes through this single seam so it
//! can be swapped for a fake in tests.

use std::collections::BTreeMap;

use bb_core::{ApiError, HttpRequest, HttpResponse, Method, Transport};

/// A `reqwest`-backed transport.
pub struct ReqwestTransport {
    client: reqwest::blocking::Client,
}

impl ReqwestTransport {
    /// Build a transport with the bb-cli user agent.
    ///
    /// # Panics
    /// Panics only if the TLS backend fails to initialize, which is fatal.
    #[must_use]
    pub fn new(user_agent: &str) -> Self {
        let client = reqwest::blocking::Client::builder()
            .user_agent(user_agent.to_owned())
            .build()
            .expect("failed to build HTTP client");
        Self { client }
    }
}

fn to_reqwest_method(m: Method) -> reqwest::Method {
    match m {
        Method::Get => reqwest::Method::GET,
        Method::Post => reqwest::Method::POST,
        Method::Put => reqwest::Method::PUT,
        Method::Delete => reqwest::Method::DELETE,
        Method::Patch => reqwest::Method::PATCH,
    }
}

impl Transport for ReqwestTransport {
    fn execute(&self, req: HttpRequest) -> Result<HttpResponse, ApiError> {
        let mut builder = self.client.request(to_reqwest_method(req.method), &req.url);
        for (k, v) in &req.headers {
            builder = builder.header(k, v);
        }
        if let Some(body) = req.body {
            builder = builder.body(body);
        }

        let resp = builder
            .send()
            .map_err(|e| ApiError::Network(e.to_string()))?;
        let status = resp.status().as_u16();

        let mut headers = BTreeMap::new();
        for (k, v) in resp.headers() {
            if let Ok(s) = v.to_str() {
                headers.insert(k.as_str().to_owned(), s.to_owned());
            }
        }

        let body = resp
            .bytes()
            .map_err(|e| ApiError::Network(e.to_string()))?
            .to_vec();

        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }
}
