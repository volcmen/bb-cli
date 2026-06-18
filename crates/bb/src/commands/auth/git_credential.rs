//! `bb auth git-credential` — a git credential helper (hidden).
//!
//! git invokes `bb auth git-credential <operation>` and passes the credential
//! attributes (`protocol=`, `host=`, …) on stdin. For `get` we emit the stored
//! username/token for that host; `store`/`erase` are no-ops (credentials are
//! managed by `bb auth login`/`logout`). Configured by `bb auth setup-git`.

use crate::auth;
use crate::core::{ConfigProvider, Context};
use clap::Args;

#[derive(Args, Debug)]
pub struct GitCredentialArgs {
    /// The git credential operation (`get` | `store` | `erase`)
    #[arg(value_name = "OPERATION")]
    pub operation: Option<String>,
}

/// Run `bb auth git-credential`.
///
/// # Errors
/// Propagates only an IO error from reading stdin; an unknown host or missing
/// credentials produce no output (git then falls back to other helpers).
pub fn run(ctx: &Context, args: GitCredentialArgs) -> anyhow::Result<()> {
    // Only `get` produces output; store/erase (and anything else) are no-ops.
    if args.operation.as_deref() != Some("get") {
        return Ok(());
    }

    let stdin = ctx.io.read_stdin_to_string()?;
    let host = parse_attr(&stdin, "host").unwrap_or_else(|| ctx.config.default_host());

    if let Some(out) = credential_response(ctx.config.as_ref(), &host) {
        ctx.io.print(&out);
    }
    Ok(())
}

/// The `key` value from git's `key=value\n` credential attribute block.
fn parse_attr(stdin: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    stdin
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .map(str::to_owned)
}

/// The git-credential `get` reply (`username=…\npassword=…\n`) for `host` from
/// stored config, or `None` when there is no stored token.
fn credential_response(config: &dyn ConfigProvider, host: &str) -> Option<String> {
    let token = config.get(host, "token")?;
    let auth_type = config
        .get(host, "auth_type")
        .unwrap_or_else(|| auth::APP_PASSWORD.to_owned());
    // OAuth bearer tokens authenticate over HTTPS as the magic `x-token-auth`
    // user; Basic auth (app password / API token) uses the stored username.
    let username = if auth_type == auth::OAUTH {
        "x-token-auth".to_owned()
    } else {
        config
            .get(host, "username")
            .unwrap_or_else(|| "x-token-auth".to_owned())
    };
    Some(format!("username={username}\npassword={token}\n"))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::api::testing::FakeTransport;
    use crate::config::FileConfig;
    use crate::core::{ConfigProvider, GitClient, Transport};
    use crate::git::{ShellGit, StubRunner};

    use super::*;
    use crate::testsupport::{test_context, ScriptedPrompter};

    const HOST: &str = "bitbucket.org";

    fn oauth_cfg() -> Arc<dyn ConfigProvider> {
        let cfg = FileConfig::blank();
        cfg.set(HOST, "auth_type", auth::OAUTH).unwrap();
        cfg.set(HOST, "token", "at-1").unwrap();
        Arc::new(cfg)
    }

    fn basic_cfg() -> Arc<dyn ConfigProvider> {
        let cfg = FileConfig::blank();
        cfg.set(HOST, "auth_type", "app_password").unwrap();
        cfg.set(HOST, "username", "alice").unwrap();
        cfg.set(HOST, "token", "pw-1").unwrap();
        Arc::new(cfg)
    }

    #[test]
    fn oauth_uses_x_token_auth() {
        let out = credential_response(oauth_cfg().as_ref(), HOST).unwrap();
        assert_eq!(out, "username=x-token-auth\npassword=at-1\n");
    }

    #[test]
    fn basic_uses_stored_username() {
        let out = credential_response(basic_cfg().as_ref(), HOST).unwrap();
        assert_eq!(out, "username=alice\npassword=pw-1\n");
    }

    #[test]
    fn unknown_host_is_none() {
        assert!(credential_response(basic_cfg().as_ref(), "example.com").is_none());
    }

    #[test]
    fn parse_attr_reads_host() {
        let stdin = "protocol=https\nhost=bitbucket.org\n\n";
        assert_eq!(parse_attr(stdin, "host").as_deref(), Some("bitbucket.org"));
        assert_eq!(parse_attr(stdin, "path"), None);
    }

    #[test]
    fn store_operation_is_noop() {
        let transport: Arc<dyn Transport> = Arc::new(FakeTransport::new());
        let git: Arc<dyn GitClient> = Arc::new(ShellGit::new(Arc::new(StubRunner::new())));
        let (ctx, bufs) = test_context(
            transport,
            git,
            basic_cfg(),
            Arc::new(ScriptedPrompter::new()),
            false,
        );
        run(
            &ctx,
            GitCredentialArgs {
                operation: Some("store".to_owned()),
            },
        )
        .unwrap();
        assert!(bufs.stdout_string().is_empty());
    }
}
