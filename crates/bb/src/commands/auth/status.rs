//! `bb auth status` — show who you're logged in as on each host.

use bb_api::models::User;
use bb_api::BitbucketClient;
use bb_core::{AuthError, Context};
use clap::Args;

use crate::auth;

#[derive(Args, Debug)]
pub struct StatusArgs {
    /// The Bitbucket host to check (default: configured hosts)
    #[arg(long)]
    pub hostname: Option<String>,
}

/// Run `bb auth status`.
///
/// Prints, for each configured host (or the one passed via `--hostname`),
/// whether the stored credentials authenticate against `GET /user`. Returns an
/// [`AuthError`] (exit 4) if any host is not logged in or fails validation.
///
/// # Errors
/// Returns [`AuthError`] when there are no hosts or any host fails to
/// authenticate; propagates other [`ApiError`](bb_core::ApiError)s.
pub fn run(ctx: &Context, args: StatusArgs) -> anyhow::Result<()> {
    let hosts: Vec<String> = match args.hostname {
        Some(h) => vec![h],
        None => ctx.config.hosts(),
    };

    if hosts.is_empty() {
        ctx.io.println(
            "You are not logged in to any Bitbucket hosts. Run `bb auth login` to authenticate.",
        );
        return Err(AuthError::new(ctx.config.default_host()).into());
    }

    let mut first_failed: Option<String> = None;

    for host in &hosts {
        let Some(header) = auth::header_for(ctx.config.as_ref(), host) else {
            ctx.io.println(&format!("X {host}: not logged in"));
            first_failed.get_or_insert_with(|| host.clone());
            continue;
        };

        let client = BitbucketClient::new(ctx.transport.clone(), Some(header));
        match client.get::<User>("/user") {
            Ok(user) => {
                ctx.io
                    .println(&format!("\u{2713} Logged in to {host} as {}", user.label()));
            }
            Err(err) if err.is_unauthorized() => {
                ctx.io.println(&format!(
                    "X {host}: authentication failed (token invalid or expired)"
                ));
                first_failed.get_or_insert_with(|| host.clone());
            }
            Err(err) => return Err(err.into()),
        }
    }

    if let Some(host) = first_failed {
        return Err(AuthError::new(host).into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bb_api::testing::FakeTransport;
    use bb_config::FileConfig;
    use bb_core::{ConfigProvider, GitClient, Method, Transport};
    use bb_git::{ShellGit, StubRunner};

    use super::*;
    use crate::testsupport::{test_context, ScriptedPrompter};

    fn git() -> Arc<dyn GitClient> {
        Arc::new(ShellGit::new(Arc::new(StubRunner::new())))
    }

    fn seeded_config() -> Arc<FileConfig> {
        let cfg = FileConfig::blank();
        cfg.set("bitbucket.org", "auth_type", "app_password")
            .unwrap();
        cfg.set("bitbucket.org", "username", "davidd").unwrap();
        cfg.set("bitbucket.org", "token", "secret").unwrap();
        Arc::new(cfg)
    }

    #[test]
    fn status_logged_in_prints_label() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "GET /user",
            FakeTransport::rest(Method::Get, "/2.0/user"),
            FakeTransport::json(200, r#"{"display_name":"David D","username":"davidd"}"#),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let config: Arc<dyn ConfigProvider> = seeded_config();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, bufs) = test_context(transport, git(), config, prompter, false);

        run(&ctx, StatusArgs { hostname: None }).unwrap();

        let out = bufs.stdout_string();
        assert!(
            out.contains("\u{2713} Logged in to bitbucket.org as David D"),
            "got: {out}"
        );
    }

    #[test]
    fn status_no_hosts_errors_with_auth() {
        let h = Arc::new(FakeTransport::new());
        let transport: Arc<dyn Transport> = h.clone();
        let config: Arc<dyn ConfigProvider> = Arc::new(FileConfig::blank());
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, bufs) = test_context(transport, git(), config, prompter, false);

        let err = run(&ctx, StatusArgs { hostname: None }).unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some());
        assert!(bufs
            .stdout_string()
            .contains("You are not logged in to any Bitbucket hosts"));
    }

    #[test]
    fn status_not_logged_in_for_host_without_creds() {
        let h = Arc::new(FakeTransport::new());
        let transport: Arc<dyn Transport> = h.clone();
        // host present but no token -> header_for returns None
        let cfg = FileConfig::blank();
        cfg.set("bitbucket.org", "auth_type", "app_password")
            .unwrap();
        let config: Arc<dyn ConfigProvider> = Arc::new(cfg);
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, bufs) = test_context(transport, git(), config, prompter, false);

        let err = run(&ctx, StatusArgs { hostname: None }).unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some());
        assert!(bufs
            .stdout_string()
            .contains("X bitbucket.org: not logged in"));
    }

    #[test]
    fn status_invalid_token_reports_failure() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "GET /user 401",
            FakeTransport::rest(Method::Get, "/2.0/user"),
            FakeTransport::json(401, r#"{"type":"error","error":{"message":"bad creds"}}"#),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let config: Arc<dyn ConfigProvider> = seeded_config();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, bufs) = test_context(transport, git(), config, prompter, false);

        let err = run(&ctx, StatusArgs { hostname: None }).unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some());
        assert!(bufs
            .stdout_string()
            .contains("X bitbucket.org: authentication failed"));
    }
}
