//! Authentication header construction (shared by `auth status` and the API
//! commands). Reads stored credentials from config and produces the
//! `Authorization` header value.

use crate::core::ConfigProvider;
use base64::Engine;

/// `auth_type` value: Atlassian API token (Basic, `email:token`).
pub const API_TOKEN: &str = "api_token";
/// `auth_type` value: app password (Basic, `username:password`).
pub const APP_PASSWORD: &str = "app_password";
/// `auth_type` value: OAuth 2.0 access token (Bearer).
pub const OAUTH: &str = "oauth";

/// A Basic `Authorization` header from `username:secret`.
#[must_use]
pub fn basic_header(username: &str, secret: &str) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(format!("{username}:{secret}"));
    format!("Basic {encoded}")
}

/// A Bearer `Authorization` header from an access token.
#[must_use]
pub fn bearer_header(token: &str) -> String {
    format!("Bearer {token}")
}

/// Build the `Authorization` header for `host` from stored config, if any
/// credentials are present.
#[must_use]
pub fn header_for(config: &dyn ConfigProvider, host: &str) -> Option<String> {
    let token = config.auth_token(host)?;
    let auth_type = config
        .get(host, "auth_type")
        .unwrap_or_else(|| APP_PASSWORD.to_owned());
    if auth_type == OAUTH {
        Some(bearer_header(&token))
    } else {
        let username = config.get(host, "username").unwrap_or_default();
        Some(basic_header(&username, &token))
    }
}

/// Bitbucket Cloud OAuth token endpoint (authorization-code + refresh grants).
pub const TOKEN_URL: &str = "https://bitbucket.org/site/oauth2/access_token";

/// The token response from Bitbucket's `/access_token` endpoint (shared by the
/// login code exchange and the background refresh grant).
#[derive(serde::Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
}

/// POST a form-encoded body to `url` with a Basic `Authorization` header and
/// decode the JSON response as `T`. On a non-2xx it surfaces Bitbucket's
/// `error_description` (invalid_grant, redirect_uri_mismatch, …) so failures are
/// diagnosable. Goes through the [`Transport`](crate::core::Transport) seam
/// directly because the client only speaks JSON request bodies.
///
/// # Errors
/// Returns an [`ApiError`](crate::core::ApiError) on a non-2xx status or a decode
/// failure.
pub fn post_form<T: serde::de::DeserializeOwned>(
    transport: &dyn crate::core::Transport,
    url: &str,
    body: &str,
    basic_auth: &str,
) -> anyhow::Result<T> {
    use crate::core::{ApiError, HttpRequest, Method};

    let req = HttpRequest::new(Method::Post, url)
        .header("Accept", "application/json")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Authorization", basic_auth.to_owned())
        .body(body.as_bytes().to_vec());

    let resp = transport.execute(req)?;
    if !resp.is_success() {
        let detail = String::from_utf8_lossy(&resp.body);
        let detail = detail.trim();
        let message = if detail.is_empty() {
            format!("token request failed with status {}", resp.status)
        } else {
            format!("token request failed with status {}: {detail}", resp.status)
        };
        return Err(ApiError::Http {
            status: resp.status,
            url: url.to_owned(),
            message,
            errors: Vec::new(),
        }
        .into());
    }
    serde_json::from_slice(&resp.body).map_err(|e| ApiError::Decode(e.to_string()).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_header_encodes() {
        // "user:pass" -> base64
        assert_eq!(basic_header("user", "pass"), "Basic dXNlcjpwYXNz");
    }

    #[test]
    fn bearer_header_formats() {
        assert_eq!(bearer_header("abc"), "Bearer abc");
    }
}
