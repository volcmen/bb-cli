//! Authentication header construction (shared by `auth status` and the API
//! commands). Reads stored credentials from config and produces the
//! `Authorization` header value.

use base64::Engine;
use bb_core::ConfigProvider;

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
