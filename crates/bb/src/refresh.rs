//! A [`Transport`] decorator that transparently refreshes an expired OAuth
//! access token and retries the request — the analog of bkt's token refresher,
//! but purely reactive (on a `401`), with no background timers.
//!
//! Wrapping at the transport seam means every command gets seamless refresh for
//! free: a bearer request that returns `401` triggers a `refresh_token` grant,
//! the new token is persisted, and the original request is retried once. If the
//! refresh isn't possible (not OAuth, no refresh token / consumer creds, or the
//! grant itself fails) the original `401` is surfaced so the caller maps it to
//! an [`AuthError`](crate::core::AuthError) telling the user to log in again.

use std::sync::Arc;

use crate::auth;
use crate::core::{ApiError, ConfigProvider, HttpRequest, HttpResponse, Transport};
use crate::render::percent_encode;

pub struct RefreshingTransport {
    inner: Arc<dyn Transport>,
    config: Arc<dyn ConfigProvider>,
}

impl RefreshingTransport {
    pub fn new(inner: Arc<dyn Transport>, config: Arc<dyn ConfigProvider>) -> Self {
        Self { inner, config }
    }

    /// Find the configured host whose *stored* OAuth token is exactly the bearer
    /// that just got a `401`. That, rather than a fixed host, is the credential
    /// we may refresh: it scopes the refresh to the host actually being talked
    /// to (so refresh works against any authenticated host, not just the
    /// default), while still guaranteeing we never touch a credential that
    /// isn't the one that failed.
    ///
    /// A bearer that differs from every stored token (a `BB_TOKEN` env override,
    /// or a freshly-minted token mid-login) isn't ours to refresh: doing so
    /// wouldn't fix this request and would clobber the stored credentials.
    fn host_owning(&self, failed_bearer: &str) -> Option<String> {
        self.config.hosts().into_iter().find(|host| {
            self.config.get(host, "auth_type").as_deref() == Some(auth::OAUTH)
                && self.config.get(host, "token").as_deref() == Some(failed_bearer)
        })
    }

    /// Exchange the stored refresh token for a new access token, persist it, and
    /// return it. `failed_bearer` is the token that just got a 401. Returns
    /// `None` when refresh is not applicable or fails — the caller then surfaces
    /// the original `401`.
    fn try_refresh(&self, failed_bearer: &str) -> Option<String> {
        let host = &self.host_owning(failed_bearer)?;
        let refresh_token = self.config.get(host, "refresh_token")?;
        let client_id = self.config.get(host, "oauth_client_id")?;
        let client_secret = self.config.get(host, "oauth_client_secret")?;

        let body = format!(
            "grant_type=refresh_token&refresh_token={}",
            percent_encode(&refresh_token)
        );
        let basic = auth::basic_header(&client_id, &client_secret);
        let token: auth::TokenResponse =
            match auth::post_form(self.inner.as_ref(), auth::TOKEN_URL, &body, &basic) {
                Ok(t) => t,
                Err(e) => {
                    // Surface *why* refresh failed (e.g. invalid_grant, or a
                    // transient 5xx/network error) rather than silently
                    // degrading to the original 401.
                    eprintln!("bb: OAuth token refresh failed: {e}");
                    return None;
                }
            };

        let _ = self.config.set(host, "token", &token.access_token);
        if let Some(rt) = &token.refresh_token {
            let _ = self.config.set(host, "refresh_token", rt);
        }
        if let Err(e) = self.config.save() {
            // The new token works for this command, but the rotated refresh
            // token wasn't written — warn so a later forced re-login is explicable.
            eprintln!("bb: refreshed the OAuth token but could not save it: {e}");
        }
        Some(token.access_token)
    }
}

impl Transport for RefreshingTransport {
    fn execute(&self, req: HttpRequest) -> Result<HttpResponse, ApiError> {
        // Only OAuth (bearer) requests are refreshable. Capture the token about
        // to be tried so we refresh only if *it* is what failed. Basic-auth
        // requests (app password / API token) pass straight through.
        let failed_bearer = req
            .headers
            .get("Authorization")
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::to_owned);
        let Some(failed_bearer) = failed_bearer else {
            return self.inner.execute(req);
        };

        let retry = req.clone();
        let resp = self.inner.execute(req)?;
        if resp.status != 401 {
            return Ok(resp);
        }

        match self.try_refresh(&failed_bearer) {
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

    use crate::config::FileConfig;
    use crate::core::Method;

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
        let t = RefreshingTransport::new(inner.clone(), config.clone());

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
        assert_eq!(seen[1].url, auth::TOKEN_URL);
        assert_eq!(
            seen[2].headers.get("Authorization").map(String::as_str),
            Some("Bearer new-access")
        );
    }

    #[test]
    fn non_401_passes_through_without_refresh() {
        let inner = Arc::new(ScriptedTransport::new(vec![resp(200, "{}")]));
        let (config, _dir) = oauth_config();
        let t = RefreshingTransport::new(inner.clone(), config);

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
        let t = RefreshingTransport::new(inner.clone(), config);

        let out = t.execute(bearer_get("old")).unwrap();
        assert_eq!(out.status, 401);
        // Only the original request — no refresh attempt.
        assert_eq!(inner.seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn basic_auth_401_is_not_refreshed() {
        let inner = Arc::new(ScriptedTransport::new(vec![resp(401, "{}")]));
        let (config, _dir) = oauth_config();
        let t = RefreshingTransport::new(inner.clone(), config);

        let req = HttpRequest::new(Method::Get, "https://api.bitbucket.org/2.0/user")
            .header("Authorization", auth::basic_header("u", "p"));
        let out = t.execute(req).unwrap();
        assert_eq!(out.status, 401);
        assert_eq!(inner.seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn refreshes_against_a_non_default_host() {
        // The authed host is NOT bitbucket.org. The refresher must still find it
        // (by matching the failed bearer to the stored token) and refresh it —
        // previously it was pinned to DEFAULT_HOST and silently surfaced the 401.
        let dir = tempfile::tempdir().unwrap();
        let cfg = FileConfig::load_from(dir.path().to_path_buf()).unwrap();
        let host = "git.example.com";
        cfg.set(host, "auth_type", "oauth").unwrap();
        cfg.set(host, "token", "old-access").unwrap();
        cfg.set(host, "refresh_token", "rt-1").unwrap();
        cfg.set(host, "oauth_client_id", "cid").unwrap();
        cfg.set(host, "oauth_client_secret", "csec").unwrap();
        let config: Arc<dyn ConfigProvider> = Arc::new(cfg);

        let inner = Arc::new(ScriptedTransport::new(vec![
            resp(401, r#"{"type":"error"}"#),
            resp(
                200,
                r#"{"access_token":"new-access","refresh_token":"rt-2"}"#,
            ),
            resp(200, r#"{"username":"davidd"}"#),
        ]));
        let t = RefreshingTransport::new(inner.clone(), config.clone());

        let out = t.execute(bearer_get("old-access")).unwrap();
        assert_eq!(out.status, 200);
        assert_eq!(config.get(host, "token").as_deref(), Some("new-access"));
        assert_eq!(inner.seen.lock().unwrap().len(), 3);
    }

    #[test]
    fn bearer_not_matching_stored_token_is_not_refreshed() {
        // The 401'd bearer differs from the stored token (e.g. a BB_TOKEN env
        // override, or a freshly-minted token mid-login). Refreshing the stored
        // creds wouldn't help this request and would clobber them, so skip it.
        let inner = Arc::new(ScriptedTransport::new(vec![resp(401, "{}")]));
        let (config, _dir) = oauth_config(); // stored token = "old-access"
        let t = RefreshingTransport::new(inner.clone(), config.clone());

        let out = t.execute(bearer_get("some-other-token")).unwrap();
        assert_eq!(out.status, 401);
        // Original request only — no refresh POST, no retry.
        assert_eq!(inner.seen.lock().unwrap().len(), 1);
        // Stored creds untouched.
        assert_eq!(
            config.get("bitbucket.org", "token").as_deref(),
            Some("old-access")
        );
        assert_eq!(
            config.get("bitbucket.org", "refresh_token").as_deref(),
            Some("rt-1")
        );
    }
}
