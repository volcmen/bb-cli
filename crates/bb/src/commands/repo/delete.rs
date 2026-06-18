//! `bb repo delete` — delete a repository (guarded by a typed confirmation).

use crate::api::BitbucketClient;
use crate::core::{AuthError, CancelError, Context, FlagError, Method, RepoId};
use clap::Args;

#[derive(Args, Debug)]
pub struct DeleteArgs {
    /// Repository as WORKSPACE/SLUG
    #[arg(value_name = "WORKSPACE/SLUG")]
    pub name: String,
    /// Skip the confirmation prompt
    #[arg(long)]
    pub yes: bool,
}

/// Run `bb repo delete`.
///
/// # Errors
/// [`AuthError`] (4) unauthenticated; [`FlagError`] (1) malformed name or a
/// non-interactive run without `--yes`; [`CancelError`] (2) when the typed
/// confirmation does not match; propagates [`ApiError`](crate::core::ApiError).
pub fn run(ctx: &Context, args: DeleteArgs) -> anyhow::Result<()> {
    let repo: RepoId = args
        .name
        .parse()
        .map_err(|e| anyhow::Error::from(FlagError::new(e)))?;
    let full = format!("{}/{}", repo.workspace(), repo.slug());

    if !args.yes {
        if !ctx.io.can_prompt() {
            return Err(FlagError::new(format!(
                "refusing to delete {full} without confirmation; pass --yes"
            ))
            .into());
        }
        let typed = ctx
            .prompter
            .input(&format!("Type {full} to confirm deletion:"), None)
            .map_err(to_anyhow)?;
        if typed.trim() != full {
            return Err(CancelError.into());
        }
    }

    let host = repo.host().to_owned();
    let Some(header) = crate::auth::header_for(ctx.config.as_ref(), &host) else {
        return Err(AuthError::new(host).into());
    };
    let client = BitbucketClient::new(ctx.transport.clone(), Some(header));

    let path = format!("/repositories/{full}");
    client.send_empty(Method::Delete, &path)?;
    ctx.io.println(&format!("✓ Deleted {full}"));
    Ok(())
}

fn to_anyhow(err: crate::core::PromptError) -> anyhow::Error {
    match err {
        crate::core::PromptError::Cancelled => CancelError.into(),
        other => anyhow::anyhow!(other),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::api::testing::FakeTransport;
    use crate::config::FileConfig;
    use crate::core::{ConfigProvider, GitClient, IoStreams, Prompter, Transport};
    use crate::git::{ShellGit, StubRunner};

    use super::*;
    use crate::testsupport::{test_context, RecordingBrowser, ScriptedPrompter};

    const HOST: &str = "bitbucket.org";

    fn authed_config() -> Arc<dyn ConfigProvider> {
        let cfg = FileConfig::blank();
        cfg.set(HOST, "auth_type", "app_password").unwrap();
        cfg.set(HOST, "username", "u").unwrap();
        cfg.set(HOST, "token", "t").unwrap();
        Arc::new(cfg)
    }

    fn git() -> Arc<dyn GitClient> {
        Arc::new(ShellGit::new(Arc::new(StubRunner::new())))
    }

    /// Non-interactive context (`can_prompt()` == false).
    fn ctx_noninteractive(
        http: Arc<FakeTransport>,
        config: Arc<dyn ConfigProvider>,
    ) -> (Context, crate::core::TestBuffers) {
        let transport: Arc<dyn Transport> = http;
        test_context(
            transport,
            git(),
            config,
            Arc::new(ScriptedPrompter::new()),
            false,
        )
    }

    /// Interactive context with a scripted `input` answer.
    fn ctx_interactive(
        http: Arc<FakeTransport>,
        config: Arc<dyn ConfigProvider>,
        prompter: Arc<dyn Prompter>,
    ) -> (Context, crate::core::TestBuffers) {
        let (mut io, bufs) = IoStreams::test();
        io.set_stdout_tty(true);
        io.set_stderr_tty(true);
        io.set_stdin_tty(true);
        io.set_never_prompt(false);
        let ctx = Context {
            io: Arc::new(io),
            prompter,
            browser: Arc::new(RecordingBrowser::default()),
            git: git(),
            config,
            transport: http,
            app_version: "test".to_owned(),
            repo_override: None,
        };
        (ctx, bufs)
    }

    fn stub_delete(h: &Arc<FakeTransport>) {
        h.stub(
            "delete repo",
            FakeTransport::rest(Method::Delete, "/repositories/acme/widgets"),
            FakeTransport::json(204, ""),
        );
    }

    #[test]
    fn delete_with_yes_deletes() {
        let h = Arc::new(FakeTransport::new());
        stub_delete(&h);
        let (ctx, bufs) = ctx_noninteractive(h.clone(), authed_config());

        run(
            &ctx,
            DeleteArgs {
                name: "acme/widgets".to_owned(),
                yes: true,
            },
        )
        .unwrap();
        assert!(bufs.stdout_string().contains("✓ Deleted acme/widgets"));
    }

    #[test]
    fn delete_confirmed_by_typing_name() {
        let h = Arc::new(FakeTransport::new());
        stub_delete(&h);
        let prompter = Arc::new(ScriptedPrompter::new().input("acme/widgets"));
        let (ctx, bufs) = ctx_interactive(h.clone(), authed_config(), prompter);

        run(
            &ctx,
            DeleteArgs {
                name: "acme/widgets".to_owned(),
                yes: false,
            },
        )
        .unwrap();
        assert!(bufs.stdout_string().contains("✓ Deleted acme/widgets"));
    }

    #[test]
    fn delete_wrong_confirmation_is_cancel() {
        let h = Arc::new(FakeTransport::new());
        let prompter = Arc::new(ScriptedPrompter::new().input("nope"));
        let (ctx, _bufs) = ctx_interactive(h.clone(), authed_config(), prompter);

        let err = run(
            &ctx,
            DeleteArgs {
                name: "acme/widgets".to_owned(),
                yes: false,
            },
        )
        .unwrap_err();
        assert!(err.downcast_ref::<CancelError>().is_some(), "got: {err}");
        assert_eq!(h.request_count(), 0);
    }

    #[test]
    fn delete_non_interactive_without_yes_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        let (ctx, _bufs) = ctx_noninteractive(h.clone(), authed_config());
        let err = run(
            &ctx,
            DeleteArgs {
                name: "acme/widgets".to_owned(),
                yes: false,
            },
        )
        .unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
        assert_eq!(h.request_count(), 0);
    }

    #[test]
    fn delete_not_authed_is_auth_error() {
        let h = Arc::new(FakeTransport::new());
        let (ctx, _bufs) = ctx_noninteractive(h.clone(), Arc::new(FileConfig::blank()));
        let err = run(
            &ctx,
            DeleteArgs {
                name: "acme/widgets".to_owned(),
                yes: true,
            },
        )
        .unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some(), "got: {err}");
    }
}
