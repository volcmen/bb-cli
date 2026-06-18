//! `bb auth switch` — change the active (default) host among logged-in hosts.
//!
//! Switching *accounts* on a single host needs multi-credential storage and is
//! out of scope (Bitbucket Cloud is one host today); this switches which
//! configured host commands default to.

use crate::core::{Context, FlagError};
use clap::Args;

#[derive(Args, Debug)]
pub struct SwitchArgs {
    /// The host to switch to (must already be logged in)
    #[arg(long)]
    pub hostname: Option<String>,
}

/// Run `bb auth switch`.
///
/// # Errors
/// [`FlagError`] (1) when the target host is not logged in, or when there is no
/// other host to switch to; [`CancelError`](crate::core::CancelError) if the
/// selection is cancelled; propagates [`ConfigError`](crate::core::ConfigError).
pub fn run(ctx: &Context, args: SwitchArgs) -> anyhow::Result<()> {
    let hosts = ctx.config.hosts();

    let target = match args.hostname {
        Some(h) => {
            if !hosts.iter().any(|x| x == &h) {
                return Err(FlagError::new(format!("not logged in to {h}")).into());
            }
            h
        }
        None => match hosts.as_slice() {
            [] => {
                return Err(FlagError::new("not logged in to any host; run `bb auth login`").into())
            }
            [_only] => {
                return Err(
                    FlagError::new("only one account configured; nothing to switch to").into(),
                )
            }
            many => {
                let idx = ctx
                    .prompter
                    .select("Switch to which host?", many)
                    .map_err(to_anyhow)?;
                many[idx].clone()
            }
        },
    };

    ctx.config.set("", "default_host", &target)?;
    ctx.config.save()?;
    ctx.io
        .println(&format!("✓ Switched active host to {target}"));
    Ok(())
}

fn to_anyhow(err: crate::core::PromptError) -> anyhow::Error {
    match err {
        crate::core::PromptError::Cancelled => crate::core::CancelError.into(),
        other => anyhow::anyhow!(other),
    }
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

    fn git() -> Arc<dyn GitClient> {
        Arc::new(ShellGit::new(Arc::new(StubRunner::new())))
    }

    fn two_hosts() -> (Arc<FileConfig>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let cfg = FileConfig::load_from(dir.path().to_path_buf()).unwrap();
        for host in ["bitbucket.org", "example.com"] {
            cfg.set(host, "auth_type", "app_password").unwrap();
            cfg.set(host, "token", "t").unwrap();
        }
        (Arc::new(cfg), dir)
    }

    fn ctx_with(
        prompter: ScriptedPrompter,
        cfg: Arc<dyn ConfigProvider>,
    ) -> (Context, crate::core::TestBuffers) {
        let transport: Arc<dyn Transport> = Arc::new(FakeTransport::new());
        test_context(transport, git(), cfg, Arc::new(prompter), false)
    }

    #[test]
    fn switch_explicit_sets_default() {
        let (cfg, _d) = two_hosts();
        let (ctx, _bufs) = ctx_with(ScriptedPrompter::new(), cfg.clone());

        run(
            &ctx,
            SwitchArgs {
                hostname: Some("example.com".to_owned()),
            },
        )
        .unwrap();
        assert_eq!(cfg.get("", "default_host").as_deref(), Some("example.com"));
    }

    #[test]
    fn switch_unknown_host_is_flag_error() {
        let (cfg, _d) = two_hosts();
        let (ctx, _bufs) = ctx_with(ScriptedPrompter::new(), cfg);

        let err = run(
            &ctx,
            SwitchArgs {
                hostname: Some("nope.com".to_owned()),
            },
        )
        .unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
    }

    #[test]
    fn switch_no_arg_multi_prompts() {
        let (cfg, _d) = two_hosts();
        // hosts() is sorted: [bitbucket.org, example.com]; select index 1.
        let (ctx, _bufs) = ctx_with(ScriptedPrompter::new().select(1), cfg.clone());

        run(&ctx, SwitchArgs { hostname: None }).unwrap();
        assert_eq!(cfg.get("", "default_host").as_deref(), Some("example.com"));
    }

    #[test]
    fn switch_no_arg_single_is_flag_error() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = FileConfig::load_from(dir.path().to_path_buf()).unwrap();
        cfg.set("bitbucket.org", "auth_type", "app_password")
            .unwrap();
        cfg.set("bitbucket.org", "token", "t").unwrap();
        let (ctx, _bufs) = ctx_with(ScriptedPrompter::new(), Arc::new(cfg));

        let err = run(&ctx, SwitchArgs { hostname: None }).unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
    }
}
