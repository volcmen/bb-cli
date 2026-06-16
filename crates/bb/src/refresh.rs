//! A [`Transport`] decorator that transparently refreshes an expired OAuth
//! access token and retries the request — the analog of bkt's token refresher,
//! but purely reactive (on a `401`), with no background timers.
//!
//! Wrapping at the transport seam means every command gets seamless refresh for
//! free: a bearer request that returns `401` triggers a `refresh_token` grant,
//! the new token is persisted, and the original request is retried once. If the
//! refresh isn't possible (not OAuth, no refresh token / consumer creds, or the
//! grant itself fails) the original `401` is surfaced so the caller maps it to
//! an [`AuthError`](bb_core::AuthError) telling the user to log in again.

use std::sync::Arc;

use bb_core::{ApiError, ConfigProvider, HttpRequest, HttpResponse, Method, Transport};

use crate::auth;
use crate::render::percent_encode;

/// Bitbucket Cloud OAuth token endpoint (refresh grants go here).
const TOKEN_URL: &str = "https://bitbucket.org/site/oauth2/access_token";

pub struct RefreshingTransport {
    inner: Arc<dyn Transport>,
    config: Arc<dyn ConfigProvider>,
    host: String,
}

#[derive(serde::Deserialize)]
struct RefreshResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
}

impl RefreshingTransport {
    pub fn new(inner: Arc<dyn Transport>, config: Arc<dyn ConfigProvider>, host: String) -> Self {
        Self {
            inner,
            config,
            host,
        }
    }

    /// Exchange the stored refresh token for a new access token, persist it, and
    /// return it. Returns `None` when refresh is not applicable or fails — the
    /// caller then surfaces the original `401`.
    fn try_refresh(&self) -> Option<String> {
        let host = &self.host;
        if self.config.get(host, "auth_type").as_deref() != Some(auth::OAUTH) {
            return None;
        }
        let refresh_token = self.config.get(host, "refresh_token")?;
        let client_id = self.config.get(host, "oauth_client_id")?;
        let client_secret = self.config.get(host, "oauth_client_secret")?;

        let body = format!(
            "grant_type=refresh_token&refresh_token={}",
            percent_encode(&refresh_token)
        );
        let req = HttpRequest::new(Method::Post, TOKEN_URL)
            .header("Accept", "application/json")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header(
                "Authorization",
                auth::basic_header(&client_id, &client_secret),
            )
            .body(body.into_bytes());

        let resp = self.inner.execute(req).ok()?;
        if !resp.is_success() {
            return None;
        }
        let parsed: RefreshResponse = serde_json::from_slice(&resp.body).ok()?;

        // Persist the rotated credentials. Best-effort: even if the save fails we
        // return the access token so the in-flight command still succeeds.
        let _ = self.config.set(host, "token", &parsed.access_token);
        if let Some(rt) = &parsed.refresh_token {
            let _ = self.config.set(host, "refresh_token", rt);
        }
        let _ = self.config.save();

        Some(parsed.access_token)
    }
}

impl Transport for RefreshingTransport {
    fn execute(&self, req: HttpRequest) -> Result<HttpResponse, ApiError> {
        // Only OAuth (bearer) requests are refreshable. Basic-auth requests
        // (app password / API token) can't be refreshed, so pass them straight
        // through.
        let is_bearer = req
            .headers
            .get("Authorization")
            .is_some_and(|v| v.starts_with("Bearer "));
        if !is_bearer {
            return self.inner.execute(req);
        }

        let retry = req.clone();
        let resp = self.inner.execute(req)?;
        if resp.status != 401 {
            return Ok(resp);
        }

        match self.try_refresh() {
            Some(new_token) => {
                let mut retry = retry;
                retry
                    .headers
                    .insert("Authorization".to_owned(), auth::bearer_header(&new_token));
                self.inner.execute(retry)
            }
            None => Ok(resp),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use bb_config::FileConfig;

    use super::*;

    /// A transport that returns scripted responses in order and records every
    /// request it received.
    struct ScriptedTransport {
        responses: Mutex<Vec<HttpResponse>>,
        seen: Mutex<Vec<HttpRequest>>,
    }

    impl ScriptedTransport {
        fn new(responses: Vec<HttpResponse>) -> Self {
            Self {
                responses: Mutex::new(responses),
                seen: Mutex::new(Vec::new()),
            }
        }
    }

    impl Transport for ScriptedTransport {
        fn execute(&self, req: HttpRequest) -> Result<HttpResponse, ApiError> {
            self.seen.lock().unwrap().push(req);
            Ok(self.responses.lock().unwrap().remove(0))
        }
    }

    fn resp(status: u16, body: &str) -> HttpResponse {
        HttpResponse {
            status,
            headers: std::collections::BTreeMap::new(),
            body: body.as_bytes().to_vec(),
        }
    }

    fn bearer_get(token: &str) -> HttpRequest {
        HttpRequest::new(Method::Get, "https://api.bitbucket.org/2.0/user")
            .header("Authorization", auth::bearer_header(token))
    }

    /// A FileConfig backed by a tempdir (so `save()` works) pre-loaded with
    /// OAuth credentials. Returns the config and keeps the tempdir alive.
    fn oauth_config() -> (Arc<dyn ConfigProvider>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let cfg = FileConfig::load_from(dir.path().to_path_buf()).unwrap();
        cfg.set("bitbucket.org", "auth_type", "oauth").unwrap();
        cfg.set("bitbucket.org", "token", "old-access").unwrap();
        cfg.set("bitbucket.org", "refresh_token", "rt-1").unwrap();
        cfg.set("bitbucket.org", "oauth_client_id", "cid").unwrap();
        cfg.set("bitbucket.org", "oauth_client_secret", "csec")
            .unwrap();
        (Arc::new(cfg), dir)
    }

    #[test]
    fn refreshes_on_401_then_retries_and_persists() {
        let inner = Arc::new(ScriptedTransport::new(vec![
            resp(401, r#"{"type":"error"}"#),
            resp(
                200,
                r#"{"access_token":"new-access","refresh_token":"rt-2"}"#,
            ),
            resp(200, r#"{"username":"davidd"}"#),
        ]));
        let (config, _dir) = oauth_config();
        let t = RefreshingTransport::new(inner.clone(), config.clone(), "bitbucket.org".to_owned());

        let out = t.execute(bearer_get("old-access")).unwrap();
        assert_eq!(out.status, 200);
        assert_eq!(out.body_str(), r#"{"username":"davidd"}"#);

        // New tokens persisted.
        assert_eq!(
            config.get("bitbucket.org", "token").as_deref(),
            Some("new-access")
        );
        assert_eq!(
            config.get("bitbucket.org", "refresh_token").as_deref(),
            Some("rt-2")
        );

        // Three inner calls: original (401), refresh POST, retry with new token.
        let seen = inner.seen.lock().unwrap();
        assert_eq!(seen.len(), 3);
        assert_eq!(seen[1].url, TOKEN_URL);
        assert_eq!(
            seen[2].headers.get("Authorization").map(String::as_str),
            Some("Bearer new-access")
        );
    }

    #[test]
    fn non_401_passes_through_without_refresh() {
        let inner = Arc::new(ScriptedTransport::new(vec![resp(200, "{}")]));
        let (config, _dir) = oauth_config();
        let t = RefreshingTransport::new(inner.clone(), config, "bitbucket.org".to_owned());

        let out = t.execute(bearer_get("old-access")).unwrap();
        assert_eq!(out.status, 200);
        assert_eq!(inner.seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn surfaces_401_when_no_refresh_token() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = FileConfig::load_from(dir.path().to_path_buf()).unwrap();
        cfg.set("bitbucket.org", "auth_type", "oauth").unwrap();
        cfg.set("bitbucket.org", "token", "old").unwrap();
        // No refresh_token / consumer creds stored.
        let config: Arc<dyn ConfigProvider> = Arc::new(cfg);
        let inner = Arc::new(ScriptedTransport::new(vec![resp(401, "{}")]));
        let t = RefreshingTransport::new(inner.clone(), config, "bitbucket.org".to_owned());

        let out = t.execute(bearer_get("old")).unwrap();
        assert_eq!(out.status, 401);
        // Only the original request — no refresh attempt.
        assert_eq!(inner.seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn basic_auth_401_is_not_refreshed() {
        let inner = Arc::new(ScriptedTransport::new(vec![resp(401, "{}")]));
        let (config, _dir) = oauth_config();
        let t = RefreshingTransport::new(inner.clone(), config, "bitbucket.org".to_owned());

        let req = HttpRequest::new(Method::Get, "https://api.bitbucket.org/2.0/user")
            .header("Authorization", auth::basic_header("u", "p"));
        let out = t.execute(req).unwrap();
        assert_eq!(out.status, 401);
        assert_eq!(inner.seen.lock().unwrap().len(), 1);
    }
}
