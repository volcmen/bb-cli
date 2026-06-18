//! `bb repo set-default` — pin the repository `bb` resolves to in this directory.
//!
//! Stores `WORKSPACE/SLUG` in `config.toml` keyed by the current directory (see
//! [`default_repo_key`]). [`Context::base_repo`](crate::core::Context::base_repo)
//! consults it after `-R` and before git-remote resolution, so commands work
//! without `-R` even when several Bitbucket remotes are present.

use crate::core::{default_repo_key, Context, FlagError, RepoId};
use clap::Args;

#[derive(Args, Debug)]
pub struct SetDefaultArgs {
    /// Repository to set as the default, as WORKSPACE/SLUG
    #[arg(value_name = "REPO")]
    pub repo: Option<String>,
    /// Print the current default for this directory
    #[arg(long)]
    pub view: bool,
    /// Clear the default for this directory
    #[arg(long)]
    pub unset: bool,
}

/// Run `bb repo set-default`.
///
/// # Errors
/// Returns [`FlagError`] (exit 1) for a malformed `REPO`, when `REPO` is not one
/// of the current Bitbucket remotes, or when no `REPO` is given and the remotes
/// cannot be narrowed to one. Propagates [`ConfigError`](crate::core::ConfigError)
/// from the save.
pub fn run(ctx: &Context, args: SetDefaultArgs) -> anyhow::Result<()> {
    let dir = std::env::current_dir()?;
    let key = default_repo_key(&dir);

    if args.view {
        match ctx.config.get("", &key).filter(|v| !v.is_empty()) {
            Some(v) => ctx.io.println(&v),
            None => ctx
                .io
                .println("no default repository set for this directory"),
        }
        return Ok(());
    }

    if args.unset {
        ctx.config.set("", &key, "")?;
        ctx.config.save()?;
        ctx.io
            .println("✓ Unset the default repository for this directory");
        return Ok(());
    }

    let target = resolve_target(ctx, args.repo.as_deref())?;

    ctx.config.set("", &key, &target.full_name())?;
    ctx.config.save()?;
    ctx.io
        .println(&format!("✓ Set default repository to {target}"));
    Ok(())
}

/// Pick the repository to pin: the parsed `REPO` (validated against the current
/// Bitbucket remotes when any exist), or — when `REPO` is omitted — the sole
/// remote, or an interactive choice among several.
fn resolve_target(ctx: &Context, repo: Option<&str>) -> anyhow::Result<RepoId> {
    let remotes = ctx.git.remotes().unwrap_or_default();

    if let Some(s) = repo {
        let target: RepoId = s
            .parse()
            .map_err(|e| anyhow::Error::from(FlagError::new(e)))?;
        if !remotes.is_empty()
            && !remotes
                .iter()
                .any(|r| r.repo.full_name() == target.full_name())
        {
            let names: Vec<String> = remotes.iter().map(|r| r.repo.full_name()).collect();
            return Err(FlagError::new(format!(
                "{} is not one of this directory's Bitbucket remotes: {}",
                target,
                names.join(", ")
            ))
            .into());
        }
        return Ok(target);
    }

    match remotes.as_slice() {
        [] => Err(FlagError::new("no Bitbucket remotes found here; pass WORKSPACE/SLUG").into()),
        [only] => Ok(only.repo.clone()),
        many => {
            let options: Vec<String> = many.iter().map(|r| r.repo.full_name()).collect();
            let idx = ctx
                .prompter
                .select("Select a default repository", &options)
                .map_err(to_anyhow)?;
            Ok(many[idx].repo.clone())
        }
    }
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
    use crate::core::{default_repo_key, ConfigProvider, GitClient, Transport};
    use crate::git::{ShellGit, StubRunner};

    use super::*;
    use crate::testsupport::{test_context, ScriptedPrompter};

    fn git_with_remotes(remotes: &str) -> Arc<dyn GitClient> {
        let stub = Arc::new(StubRunner::new());
        stub.register(r"^git remote -v$", 0, remotes);
        Arc::new(ShellGit::new(stub))
    }

    fn git_no_calls() -> Arc<dyn GitClient> {
        Arc::new(ShellGit::new(Arc::new(StubRunner::new())))
    }

    const TWO_REMOTES: &str = "origin\tgit@bitbucket.org:me/widgets.git (fetch)\n\
         origin\tgit@bitbucket.org:me/widgets.git (push)\n\
         upstream\tgit@bitbucket.org:acme/widgets.git (fetch)\n\
         upstream\tgit@bitbucket.org:acme/widgets.git (push)\n";

    fn ctx_with(
        git: Arc<dyn GitClient>,
        prompter: ScriptedPrompter,
        cfg: Arc<dyn ConfigProvider>,
    ) -> (Context, crate::core::TestBuffers) {
        let transport: Arc<dyn Transport> = Arc::new(FakeTransport::new());
        test_context(transport, git, cfg, Arc::new(prompter), false)
    }

    fn temp_config() -> (Arc<FileConfig>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Arc::new(FileConfig::load_from(dir.path().to_path_buf()).unwrap());
        (cfg, dir)
    }

    fn stored(cfg: &FileConfig) -> Option<String> {
        let dir = std::env::current_dir().unwrap();
        cfg.get("", &default_repo_key(&dir))
            .filter(|v| !v.is_empty())
    }

    fn args() -> SetDefaultArgs {
        SetDefaultArgs {
            repo: None,
            view: false,
            unset: false,
        }
    }

    #[test]
    fn set_default_explicit_repo_writes_config() {
        let (cfg, _d) = temp_config();
        let (ctx, bufs) = ctx_with(
            git_with_remotes(TWO_REMOTES),
            ScriptedPrompter::new(),
            cfg.clone(),
        );

        run(
            &ctx,
            SetDefaultArgs {
                repo: Some("acme/widgets".to_owned()),
                ..args()
            },
        )
        .unwrap();

        assert_eq!(stored(&cfg).as_deref(), Some("acme/widgets"));
        assert!(bufs
            .stdout_string()
            .contains("✓ Set default repository to acme/widgets"));
    }

    #[test]
    fn set_default_rejects_repo_not_a_remote() {
        let (cfg, _d) = temp_config();
        let (ctx, _bufs) = ctx_with(
            git_with_remotes(TWO_REMOTES),
            ScriptedPrompter::new(),
            cfg.clone(),
        );

        let err = run(
            &ctx,
            SetDefaultArgs {
                repo: Some("other/thing".to_owned()),
                ..args()
            },
        )
        .unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
        assert_eq!(stored(&cfg), None);
    }

    #[test]
    fn set_default_no_arg_single_remote_uses_it() {
        let (cfg, _d) = temp_config();
        let one = "origin\tgit@bitbucket.org:acme/widgets.git (fetch)\n\
                   origin\tgit@bitbucket.org:acme/widgets.git (push)\n";
        let (ctx, _bufs) = ctx_with(git_with_remotes(one), ScriptedPrompter::new(), cfg.clone());

        run(&ctx, args()).unwrap();
        assert_eq!(stored(&cfg).as_deref(), Some("acme/widgets"));
    }

    #[test]
    fn set_default_no_arg_multi_remote_prompts() {
        let (cfg, _d) = temp_config();
        // remotes() sorts origin(me/widgets) before upstream(acme/widgets);
        // select index 1 -> upstream.
        let (ctx, _bufs) = ctx_with(
            git_with_remotes(TWO_REMOTES),
            ScriptedPrompter::new().select(1),
            cfg.clone(),
        );

        run(&ctx, args()).unwrap();
        assert_eq!(stored(&cfg).as_deref(), Some("acme/widgets"));
    }

    #[test]
    fn set_default_view_reports_current() {
        let (cfg, _d) = temp_config();
        let dir = std::env::current_dir().unwrap();
        cfg.set("", &default_repo_key(&dir), "acme/widgets")
            .unwrap();
        let (ctx, bufs) = ctx_with(git_no_calls(), ScriptedPrompter::new(), cfg.clone());

        run(
            &ctx,
            SetDefaultArgs {
                view: true,
                ..args()
            },
        )
        .unwrap();
        assert!(bufs.stdout_string().contains("acme/widgets"));
    }

    #[test]
    fn set_default_unset_clears() {
        let (cfg, _d) = temp_config();
        let dir = std::env::current_dir().unwrap();
        cfg.set("", &default_repo_key(&dir), "acme/widgets")
            .unwrap();
        let (ctx, _bufs) = ctx_with(git_no_calls(), ScriptedPrompter::new(), cfg.clone());

        run(
            &ctx,
            SetDefaultArgs {
                unset: true,
                ..args()
            },
        )
        .unwrap();
        assert_eq!(stored(&cfg), None);
    }
}
