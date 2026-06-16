//! `bb auth login` — Basic (token paste) and OAuth 2.0 (`--web`).

use std::io::{Read, Write};
use std::net::TcpListener;

use bb_api::models::User;
use bb_api::BitbucketClient;
use bb_core::{Context, FlagError};
use clap::Args;

use crate::auth;

#[derive(Args, Debug)]
pub struct LoginArgs {
    /// The Bitbucket host (default: bitbucket.org)
    #[arg(long)]
    pub hostname: Option<String>,
    /// Authenticate via OAuth 2.0 in the browser
    #[arg(long)]
    pub web: bool,
    /// Read the token / app password from standard input
    #[arg(long)]
    pub with_token: bool,
    /// Username (app password) or account email (API token)
    #[arg(long)]
    pub username: Option<String>,
    /// Credential type for Basic auth
    #[arg(long, value_parser = ["api_token", "app_password"])]
    pub auth_type: Option<String>,
    /// OAuth consumer key for `--web` (else $BB_OAUTH_CLIENT_ID, else stored)
    #[arg(long)]
    pub client_id: Option<String>,
    /// OAuth consumer secret for `--web` (else $BB_OAUTH_CLIENT_SECRET, else stored)
    #[arg(long)]
    pub client_secret: Option<String>,
}

/// Run `bb auth login`.
///
/// # Errors
/// Returns [`FlagError`] for invalid credentials or missing non-interactive
/// inputs, [`CancelError`](bb_core::CancelError) if a prompt is cancelled, and
/// propagates other [`ApiError`](bb_core::ApiError)s.
pub fn run(ctx: &Context, args: LoginArgs) -> anyhow::Result<()> {
    let host = args
        .hostname
        .clone()
        .unwrap_or_else(|| ctx.config.default_host());

    if args.web {
        oauth_login(ctx, &host, &args)
    } else {
        basic_login(ctx, &host, &args)
    }
}

// ----- Basic (token paste) -----------------------------------------------

fn basic_login(ctx: &Context, host: &str, args: &LoginArgs) -> anyhow::Result<()> {
    let can_prompt = ctx.io.can_prompt();

    // Resolve auth_type.
    let auth_type = if let Some(t) = &args.auth_type {
        t.clone()
    } else if can_prompt {
        let options = vec![auth::API_TOKEN.to_owned(), auth::APP_PASSWORD.to_owned()];
        let idx = ctx
            .prompter
            .select("How would you like to authenticate?", &options)
            .map_err(to_anyhow)?;
        options[idx].clone()
    } else {
        auth::APP_PASSWORD.to_owned()
    };

    let is_api_token = auth_type == auth::API_TOKEN;

    // Resolve username/email.
    let username = if let Some(u) = &args.username {
        u.clone()
    } else if can_prompt {
        let label = if is_api_token {
            "Atlassian account email"
        } else {
            "Bitbucket username"
        };
        ctx.prompter.input(label, None).map_err(to_anyhow)?
    } else {
        return Err(FlagError::new(format!(
            "to log in non-interactively to {host}, pass --username and --with-token (and --auth-type)"
        ))
        .into());
    };

    // Resolve secret.
    let secret = if args.with_token {
        ctx.io.read_stdin_to_string()?.trim().to_owned()
    } else if can_prompt {
        let label = if is_api_token {
            "API token"
        } else {
            "App password"
        };
        ctx.prompter.password(label).map_err(to_anyhow)?
    } else {
        return Err(FlagError::new(format!(
            "to log in non-interactively to {host}, pass --with-token to read the secret from stdin"
        ))
        .into());
    };

    if secret.trim().is_empty() {
        return Err(FlagError::new("no token was provided").into());
    }

    // Validate the credentials before saving anything.
    let header = auth::basic_header(&username, &secret);
    let client = BitbucketClient::new(ctx.transport.clone(), Some(header));
    let user: User = match client.get::<User>("/user") {
        Ok(u) => u,
        Err(err) if err.is_unauthorized() => {
            // Surface the server's actual message — e.g. an Atlassian API token
            // without Bitbucket scopes returns "not supported for this endpoint".
            let detail = match &err {
                bb_core::ApiError::Http { message, .. } => message.as_str(),
                _ => "unauthorized",
            };
            return Err(FlagError::new(format!(
                "could not authenticate to {host}: {detail}\n\
                 hint: for an app password, the username is your Bitbucket username; for an \
                 Atlassian API token, the username is your account email and the token must be \
                 created with Bitbucket scopes."
            ))
            .into());
        }
        Err(err) => return Err(err.into()),
    };

    // Persist.
    ctx.config.set(host, "auth_type", &auth_type)?;
    ctx.config.set(host, "username", &username)?;
    ctx.config.set(host, "token", &secret)?;
    ctx.config.save()?;

    ctx.io
        .println(&format!("\u{2713} Logged in to {host} as {}", user.label()));
    Ok(())
}

// ----- OAuth 2.0 (--web) --------------------------------------------------

fn oauth_login(ctx: &Context, host: &str, args: &LoginArgs) -> anyhow::Result<()> {
    // OAuth endpoints are Bitbucket Cloud-only.
    if host != bb_core::DEFAULT_HOST {
        return Err(FlagError::new(format!(
            "OAuth login (--web) is only supported on bitbucket.org; \
             Data Center OAuth for {host} is not supported yet. \
             Use `bb auth login --hostname {host}` with an app password or API token instead."
        ))
        .into());
    }

    // Consumer credentials: --client-id/--secret flags → env → embedded
    // (baked at build time) → previously-stored config.
    let env_nonempty = |k: &str| std::env::var(k).ok().filter(|s| !s.is_empty());
    let client_id = args
        .client_id
        .clone()
        .or_else(|| env_nonempty("BB_OAUTH_CLIENT_ID"))
        .or_else(embedded_client_id)
        .or_else(|| ctx.config.get(host, "oauth_client_id"));
    let client_secret = args
        .client_secret
        .clone()
        .or_else(|| env_nonempty("BB_OAUTH_CLIENT_SECRET"))
        .or_else(embedded_client_secret)
        .or_else(|| ctx.config.get(host, "oauth_client_secret"));

    let (Some(client_id), Some(client_secret)) = (client_id, client_secret) else {
        return Err(FlagError::new(
            "OAuth login (--web) needs a one-time OAuth consumer:\n\
             1. Open https://bitbucket.org/<workspace>/workspace/settings/api and click \"Add consumer\".\n\
             2. Set the Callback URL to exactly: http://127.0.0.1/callback\n\
             3. Grant permissions: Account, Repositories, Pull requests (read/write as needed).\n\
             4. Save, copy the Key and Secret, then run:\n\
             \u{20}     bb auth login --web --client-id <KEY> --client-secret <SECRET>\n\
             bb stores them, so later `bb auth login --web` just works. \
             (Or export BB_OAUTH_CLIENT_ID / BB_OAUTH_CLIENT_SECRET, or bake them in at build time.)"
        )
        .into());
    };

    // Loopback callback on a random port. Bitbucket does RFC 8252 loopback
    // matching for 127.0.0.1, so a consumer callback of `http://127.0.0.1/callback`
    // matches any port — nothing to reserve, no conflicts.
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| FlagError::new(format!("could not start the OAuth callback server: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| FlagError::new(format!("could not read the callback port: {e}")))?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");

    // CSRF protection (state) + PKCE (S256) for defense-in-depth.
    let state = random_state()?;
    let pkce = generate_pkce()?;

    let authorize_url = format!(
        "https://bitbucket.org/site/oauth2/authorize\
         ?client_id={}&response_type=code&redirect_uri={}&state={}\
         &code_challenge={}&code_challenge_method=S256",
        url_encode(&client_id),
        url_encode(&redirect_uri),
        url_encode(&state),
        url_encode(&pkce.challenge),
    );

    ctx.io
        .eprintln("Opening your browser to authorize bb with Bitbucket...");
    ctx.io
        .eprintln(&format!("If it does not open, visit:\n  {authorize_url}"));
    let _ = ctx.browser.browse(&authorize_url);

    // Wait for the redirect carrying the auth code (and verify the CSRF state).
    let code = wait_for_code(&listener, &state)?;

    let client = BitbucketClient::new(ctx.transport.clone(), None);
    let label = exchange_and_store(
        ctx,
        &client,
        host,
        &client_id,
        &client_secret,
        &code,
        &redirect_uri,
        &pkce.verifier,
    )?;

    ctx.io
        .println(&format!("\u{2713} Logged in to {host} as {label}"));
    Ok(())
}

/// Accept one connection on `listener`, parse the `code` and `state` query
/// parameters from the HTTP GET request line, verify `state` matches the value
/// we sent (CSRF protection), write a friendly response, and return the code.
fn wait_for_code(listener: &TcpListener, expected_state: &str) -> anyhow::Result<String> {
    let (mut stream, _) = listener.accept()?;
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf)?;
    let request = String::from_utf8_lossy(&buf[..n]);

    // First line: "GET /callback?code=XXXX&state=YYYY HTTP/1.1"
    let first_line = request.lines().next().unwrap_or_default();
    let target = first_line.split_whitespace().nth(1).unwrap_or_default();

    let body = "You may close this tab and return to the terminal.";
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();

    // Verify the CSRF state before trusting the code.
    let returned_state = query_param(target, "state").unwrap_or_default();
    if returned_state != expected_state {
        return Err(FlagError::new("OAuth state mismatch; aborting login").into());
    }

    let code = query_param(target, "code").ok_or_else(|| {
        FlagError::new("no authorization code was returned by Bitbucket; login aborted")
    })?;

    Ok(code)
}

/// Extract a query parameter `key` from a request target like
/// `/callback?code=ABC&state=XYZ`.
fn query_param(target: &str, key: &str) -> Option<String> {
    let query = target.split_once('?').map(|(_, q)| q)?;
    let prefix = format!("{key}=");
    for pair in query.split('&') {
        if let Some(value) = pair.strip_prefix(&prefix) {
            if !value.is_empty() {
                return Some(value.to_owned());
            }
        }
    }
    None
}

/// A random hex-encoded opaque value for the OAuth `state` parameter.
fn random_state() -> anyhow::Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).map_err(|e| anyhow::anyhow!(e))?;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    Ok(s)
}

/// A PKCE verifier + its S256 challenge (RFC 7636).
struct Pkce {
    verifier: String,
    challenge: String,
}

fn generate_pkce() -> anyhow::Result<Pkce> {
    use base64::Engine as _;
    use sha2::{Digest, Sha256};
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|e| anyhow::anyhow!(e))?;
    let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(verifier.as_bytes()));
    Ok(Pkce {
        verifier,
        challenge,
    })
}

/// OAuth consumer credentials baked in at build time (see `build.rs`). `None`
/// for source builds compiled without `BB_OAUTH_CLIENT_ID`/`SECRET` in the env.
fn embedded_client_id() -> Option<String> {
    option_env!("BB_EMBED_OAUTH_CLIENT_ID")
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn embedded_client_secret() -> Option<String> {
    option_env!("BB_EMBED_OAUTH_CLIENT_SECRET")
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// The token-exchange response from Bitbucket's `/access_token` endpoint.
#[derive(serde::Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
}

/// Exchange the authorization `code` for tokens, validate via `GET /user`, and
/// persist the OAuth credentials. Returns the authenticated user's label.
///
/// Factored out so it can be unit-tested with a `FakeTransport` (no listener or
/// browser involved).
#[allow(clippy::too_many_arguments)]
fn exchange_and_store(
    ctx: &Context,
    client: &BitbucketClient,
    host: &str,
    client_id: &str,
    client_secret: &str,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> anyhow::Result<String> {
    let form = TokenForm {
        grant_type: "authorization_code",
        code,
        redirect_uri,
        code_verifier,
    };
    let basic = auth::basic_header(client_id, client_secret);
    let token: TokenResponse = post_form(
        ctx.transport.as_ref(),
        "https://bitbucket.org/site/oauth2/access_token",
        &form.encode(),
        &basic,
    )?;

    // Validate with the access token.
    let bearer = auth::bearer_header(&token.access_token);
    let authed = BitbucketClient::new(ctx.transport.clone(), Some(bearer));
    let user: User = authed.get::<User>("/user")?;
    let _ = client; // reserved for future refresh flows

    ctx.config.set(host, "auth_type", auth::OAUTH)?;
    ctx.config.set(host, "token", &token.access_token)?;
    if let Some(refresh) = &token.refresh_token {
        ctx.config.set(host, "refresh_token", refresh)?;
    }
    ctx.config.set(host, "oauth_client_id", client_id)?;
    ctx.config.set(host, "oauth_client_secret", client_secret)?;
    ctx.config.save()?;

    Ok(user.label())
}

struct TokenForm<'a> {
    grant_type: &'a str,
    code: &'a str,
    redirect_uri: &'a str,
    code_verifier: &'a str,
}

impl TokenForm<'_> {
    fn encode(&self) -> String {
        format!(
            "grant_type={}&code={}&redirect_uri={}&code_verifier={}",
            url_encode(self.grant_type),
            url_encode(self.code),
            url_encode(self.redirect_uri),
            url_encode(self.code_verifier),
        )
    }
}

/// Minimal percent-encoding for `application/x-www-form-urlencoded` values.
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// POST a form-encoded body to `url` with a Basic `Authorization` header and
/// decode the JSON response as `T`. Goes through the [`Transport`] seam directly
/// because [`BitbucketClient`] only speaks JSON request bodies.
fn post_form<T: serde::de::DeserializeOwned>(
    transport: &dyn bb_core::Transport,
    url: &str,
    body: &str,
    basic_auth: &str,
) -> anyhow::Result<T> {
    use bb_core::{ApiError, HttpRequest, Method};

    let req = HttpRequest::new(Method::Post, url)
        .header("Accept", "application/json")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Authorization", basic_auth.to_owned())
        .body(body.as_bytes().to_vec());

    let resp = transport.execute(req)?;
    if !resp.is_success() {
        return Err(ApiError::Http {
            status: resp.status,
            url: url.to_owned(),
            message: format!("token exchange failed with status {}", resp.status),
            errors: Vec::new(),
        }
        .into());
    }
    serde_json::from_slice(&resp.body).map_err(|e| ApiError::Decode(e.to_string()).into())
}

fn to_anyhow(err: bb_core::PromptError) -> anyhow::Error {
    match err {
        bb_core::PromptError::Cancelled => bb_core::CancelError.into(),
        other => anyhow::anyhow!(other),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bb_api::testing::FakeTransport;
    use bb_config::FileConfig;
    use bb_core::{ConfigProvider, GitClient, Method, Prompter, Transport};
    use bb_core::{Context, IoStreams, TestBuffers};
    use bb_git::{ShellGit, StubRunner};

    use super::*;
    use crate::testsupport::{test_context, RecordingBrowser, ScriptedPrompter};

    fn git() -> Arc<dyn GitClient> {
        Arc::new(ShellGit::new(Arc::new(StubRunner::new())))
    }

    /// Like `test_context`, but with interactive prompting enabled (a TTY where
    /// `can_prompt()` is true) so the password-prompt branch is exercised.
    /// `test_context` leaves `never_prompt` set, so we build the context here.
    fn interactive_context(
        transport: Arc<dyn Transport>,
        git: Arc<dyn GitClient>,
        config: Arc<dyn ConfigProvider>,
        prompter: Arc<dyn Prompter>,
    ) -> (Context, TestBuffers) {
        let (mut io, bufs) = IoStreams::test();
        io.set_stdout_tty(true);
        io.set_stderr_tty(true);
        io.set_stdin_tty(true);
        io.set_never_prompt(false);
        let ctx = Context {
            io: Arc::new(io),
            prompter,
            browser: Arc::new(RecordingBrowser::default()),
            git,
            config,
            transport,
            app_version: "test".to_owned(),
            repo_override: None,
        };
        (ctx, bufs)
    }

    fn args() -> LoginArgs {
        LoginArgs {
            hostname: None,
            web: false,
            with_token: false,
            username: None,
            auth_type: None,
            client_id: None,
            client_secret: None,
        }
    }

    #[test]
    fn basic_login_happy_path_saves_and_prints() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "GET /user",
            FakeTransport::rest(Method::Get, "/2.0/user"),
            FakeTransport::json(200, r#"{"display_name":"David D","username":"davidd"}"#),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let dir = tempfile::tempdir().unwrap();
        let cfg = Arc::new(FileConfig::load_from(dir.path().to_path_buf()).unwrap());
        let config: Arc<dyn ConfigProvider> = cfg.clone();
        // Interactive: the secret comes from the password prompt, not stdin.
        let prompter = Arc::new(ScriptedPrompter::new().password("s3cret"));
        let (ctx, bufs) = interactive_context(transport, git(), config, prompter);

        let a = LoginArgs {
            username: Some("davidd".to_owned()),
            auth_type: Some("app_password".to_owned()),
            ..args() // with_token = false -> prompt path
        };
        run(&ctx, a).unwrap();

        assert_eq!(
            cfg.get("bitbucket.org", "auth_type").as_deref(),
            Some("app_password")
        );
        assert_eq!(
            cfg.get("bitbucket.org", "username").as_deref(),
            Some("davidd")
        );
        assert_eq!(cfg.get("bitbucket.org", "token").as_deref(), Some("s3cret"));
        assert!(bufs
            .stdout_string()
            .contains("\u{2713} Logged in to bitbucket.org as David D"));
    }

    #[test]
    fn basic_login_empty_secret_is_rejected() {
        let h = Arc::new(FakeTransport::new());
        // No /user stub: an empty secret must be rejected before validation.
        let transport: Arc<dyn Transport> = h.clone();
        let cfg = Arc::new(FileConfig::blank());
        let config: Arc<dyn ConfigProvider> = cfg.clone();
        // Interactive prompt path; the password is blank/whitespace.
        let prompter = Arc::new(ScriptedPrompter::new().password("   "));
        let (ctx, _bufs) = interactive_context(transport, git(), config, prompter);

        let a = LoginArgs {
            username: Some("davidd".to_owned()),
            auth_type: Some("app_password".to_owned()),
            ..args()
        };
        let err = run(&ctx, a).unwrap_err();
        let flag = err.downcast_ref::<FlagError>().expect("FlagError");
        assert!(flag.to_string().contains("no token was provided"));
        assert!(cfg.hosts().is_empty());
    }

    #[test]
    fn basic_login_invalid_creds_does_not_save() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "GET /user 401",
            FakeTransport::rest(Method::Get, "/2.0/user"),
            FakeTransport::json(401, r#"{"type":"error","error":{"message":"bad"}}"#),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let cfg = Arc::new(FileConfig::blank());
        let config: Arc<dyn ConfigProvider> = cfg.clone();
        // Interactive prompt supplies a non-empty (but invalid) secret so the
        // request reaches the API and gets a 401.
        let prompter = Arc::new(ScriptedPrompter::new().password("bad-secret"));
        let (ctx, _bufs) = interactive_context(transport, git(), config, prompter);

        let a = LoginArgs {
            username: Some("davidd".to_owned()),
            auth_type: Some("app_password".to_owned()),
            ..args()
        };
        let err = run(&ctx, a).unwrap_err();
        let flag = err.downcast_ref::<FlagError>().expect("FlagError");
        // Surfaces the failure and the server's detail message ("bad").
        assert!(flag.to_string().contains("could not authenticate"));
        assert!(flag.to_string().contains("bad"));
        // nothing persisted
        assert!(cfg.get("bitbucket.org", "token").is_none());
        assert!(cfg.hosts().is_empty());
    }

    #[test]
    fn non_interactive_missing_secret_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        let transport: Arc<dyn Transport> = h.clone();
        let config: Arc<dyn ConfigProvider> = Arc::new(FileConfig::blank());
        let prompter = Arc::new(ScriptedPrompter::new());
        // tty=false -> can_prompt() is false (never_prompt set in test IoStreams)
        let (ctx, _bufs) = test_context(transport, git(), config, prompter, false);

        let a = LoginArgs {
            username: Some("davidd".to_owned()),
            auth_type: Some("app_password".to_owned()),
            ..args() // with_token = false
        };
        let err = run(&ctx, a).unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some());
    }

    #[test]
    fn oauth_rejects_non_cloud_host() {
        let h = Arc::new(FakeTransport::new());
        let transport: Arc<dyn Transport> = h.clone();
        let config: Arc<dyn ConfigProvider> = Arc::new(FileConfig::blank());
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, _bufs) = test_context(transport, git(), config, prompter, false);

        let a = LoginArgs {
            web: true,
            hostname: Some("bitbucket.example.com".to_owned()),
            ..args()
        };
        let err = run(&ctx, a).unwrap_err();
        let flag = err.downcast_ref::<FlagError>().expect("FlagError");
        assert!(
            flag.to_string().contains("only supported on bitbucket.org"),
            "got: {flag}"
        );
    }

    #[test]
    fn oauth_exchange_and_store_persists_oauth_creds() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "token exchange",
            FakeTransport::rest(Method::Post, "/site/oauth2/access_token"),
            FakeTransport::json(
                200,
                r#"{"access_token":"at-123","refresh_token":"rt-456","token_type":"bearer"}"#,
            ),
        );
        h.stub(
            "GET /user",
            FakeTransport::rest(Method::Get, "/2.0/user"),
            FakeTransport::json(200, r#"{"display_name":"David D","username":"davidd"}"#),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let dir = tempfile::tempdir().unwrap();
        let cfg = Arc::new(FileConfig::load_from(dir.path().to_path_buf()).unwrap());
        let config: Arc<dyn ConfigProvider> = cfg.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, _bufs) = test_context(transport.clone(), git(), config, prompter, false);

        let client = BitbucketClient::new(transport, None);
        let label = exchange_and_store(
            &ctx,
            &client,
            "bitbucket.org",
            "cid",
            "csecret",
            "the-code",
            "http://127.0.0.1:5000/callback",
            "pkce-verifier-xyz",
        )
        .unwrap();

        assert_eq!(label, "David D");
        assert_eq!(
            cfg.get("bitbucket.org", "auth_type").as_deref(),
            Some("oauth")
        );
        assert_eq!(cfg.get("bitbucket.org", "token").as_deref(), Some("at-123"));
        assert_eq!(
            cfg.get("bitbucket.org", "refresh_token").as_deref(),
            Some("rt-456")
        );
        assert_eq!(
            cfg.get("bitbucket.org", "oauth_client_id").as_deref(),
            Some("cid")
        );

        // Verify the token-exchange request carried the form body + basic auth.
        let reqs = h.requests.lock().unwrap();
        let exch = reqs
            .iter()
            .find(|r| r.url.contains("/site/oauth2/access_token"))
            .expect("exchange request");
        let body = String::from_utf8_lossy(exch.body.as_deref().unwrap_or_default());
        assert!(body.contains("grant_type=authorization_code"));
        assert!(body.contains("code=the-code"));
        assert!(body.contains("code_verifier=pkce-verifier-xyz"));
        assert_eq!(
            cfg.get("bitbucket.org", "oauth_client_secret").as_deref(),
            Some("csecret")
        );
        assert!(exch
            .headers
            .get("Authorization")
            .is_some_and(|h| h.starts_with("Basic ")));
    }

    #[test]
    fn query_param_extracts_code_and_state() {
        assert_eq!(
            query_param("/callback?code=abc123&state=xyz", "code"),
            Some("abc123".to_owned())
        );
        assert_eq!(
            query_param("/callback?code=abc123&state=xyz", "state"),
            Some("xyz".to_owned())
        );
        assert_eq!(query_param("/callback", "code"), None);
        assert_eq!(query_param("/callback?state=x", "code"), None);
    }
}
