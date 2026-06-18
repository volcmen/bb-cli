//! `bb repo sync` — fast-forward the current branch of a fork from its upstream.

use crate::api::models::Repository;
use crate::api::BitbucketClient;
use crate::core::{AuthError, Context, FlagError, RepoId};
use clap::Args;

#[derive(Args, Debug)]
pub struct SyncArgs {
    /// Upstream source as WORKSPACE/SLUG (default: the fork's parent)
    #[arg(long, value_name = "SOURCE")]
    pub source: Option<String>,
}

/// Run `bb repo sync`.
///
/// # Errors
/// Returns [`AuthError`] (exit 4) when the parent must be looked up but the user
/// is unauthenticated, [`FlagError`] (exit 1) for a malformed `--source` or when
/// the repo is not a fork (and no `--source` was given), and propagates
/// [`GitError`](crate::core::GitError) from the fetch / fast-forward.
pub fn run(ctx: &Context, args: SyncArgs) -> anyhow::Result<()> {
    let repo = ctx.base_repo()?;
    let host = repo.host().to_owned();

    let source = resolve_source(ctx, &repo, &host, args.source.as_deref())?;

    let protocol = ctx
        .config
        .get("", "git_protocol")
        .unwrap_or_else(|| "https".to_owned());
    let url = build_remote_url(&host, &source, &protocol);

    let branch = ctx.git.current_branch()?;
    ctx.git.fetch(&url, &branch)?;
    ctx.git.merge_ff("FETCH_HEAD")?;

    ctx.io
        .println(&format!("✓ Synced {repo} with {source} ({branch})"));
    Ok(())
}

/// Resolve the upstream source: the parsed `--source`, else the fork's `parent`
/// (one authenticated `GET` on the current repo).
fn resolve_source(
    ctx: &Context,
    repo: &RepoId,
    host: &str,
    source: Option<&str>,
) -> anyhow::Result<RepoId> {
    if let Some(s) = source {
        return s
            .parse()
            .map_err(|e| anyhow::Error::from(FlagError::new(e)));
    }

    let Some(header) = crate::auth::header_for(ctx.config.as_ref(), host) else {
        return Err(AuthError::new(host.to_owned()).into());
    };
    let client = BitbucketClient::new(ctx.transport.clone(), Some(header));

    let path = format!("/repositories/{}/{}", repo.workspace(), repo.slug());
    let current: Repository = client.get(&path)?;

    match current.parent.and_then(|p| p.full_name) {
        Some(full_name) => full_name
            .parse()
            .map_err(|e| anyhow::Error::from(FlagError::new(e))),
        None => Err(FlagError::new(format!(
            "{repo} is not a fork; pass --source WORKSPACE/SLUG"
        ))
        .into()),
    }
}

/// Build a Bitbucket clone URL from `host` + repo for the given protocol
/// (`"ssh"` → `git@host:ws/slug.git`, else `https://host/ws/slug.git`).
fn build_remote_url(host: &str, repo: &RepoId, protocol: &str) -> String {
    if protocol == "ssh" {
        format!("git@{host}:{}/{}.git", repo.workspace(), repo.slug())
    } else {
        format!("https://{host}/{}/{}.git", repo.workspace(), repo.slug())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::api::testing::FakeTransport;
    use crate::config::FileConfig;
    use crate::core::{ConfigProvider, GitClient, Method, RepoId, Transport};
    use crate::git::{ShellGit, StubRunner};

    use super::*;
    use crate::testsupport::{test_context, ScriptedPrompter};

    const HOST: &str = "bitbucket.org";

    fn authed_config() -> Arc<dyn ConfigProvider> {
        let cfg = FileConfig::blank();
        cfg.set(HOST, "auth_type", "app_password").unwrap();
        cfg.set(HOST, "username", "u").unwrap();
        cfg.set(HOST, "token", "t").unwrap();
        Arc::new(cfg)
    }

    fn ctx_with(
        http: Arc<FakeTransport>,
        git: Arc<dyn GitClient>,
        cfg: Arc<dyn ConfigProvider>,
    ) -> (Context, crate::core::TestBuffers) {
        let transport: Arc<dyn Transport> = http;
        let (mut ctx, bufs) = test_context(
            transport,
            git,
            cfg,
            Arc::new(ScriptedPrompter::new()),
            false,
        );
        // The fork we are syncing.
        ctx.repo_override = Some(RepoId::new("me", "widgets"));
        (ctx, bufs)
    }

    /// A git stub that expects a fast-forward from the given https source URL.
    fn git_syncing(source_url_re: &str) -> Arc<dyn GitClient> {
        let stub = Arc::new(StubRunner::new());
        stub.register(r"^git rev-parse --abbrev-ref HEAD$", 0, "main\n");
        stub.register(source_url_re, 0, "");
        stub.register(r"^git merge --ff-only FETCH_HEAD$", 0, "");
        Arc::new(ShellGit::new(stub))
    }

    fn git_no_calls() -> Arc<dyn GitClient> {
        Arc::new(ShellGit::new(Arc::new(StubRunner::new())))
    }

    #[test]
    fn sync_with_source_fetches_and_ff() {
        let h = Arc::new(FakeTransport::new()); // no API call on the --source path
        let git = git_syncing(r"^git fetch https://bitbucket\.org/acme/widgets\.git main$");
        let (ctx, bufs) = ctx_with(h, git, Arc::new(FileConfig::blank()));

        run(
            &ctx,
            SyncArgs {
                source: Some("acme/widgets".to_owned()),
            },
        )
        .unwrap();

        assert!(bufs
            .stdout_string()
            .contains("✓ Synced me/widgets with acme/widgets (main)"));
    }

    #[test]
    fn sync_autodetects_parent() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get fork",
            FakeTransport::rest(Method::Get, "/repositories/me/widgets"),
            FakeTransport::json(
                200,
                r#"{"full_name":"me/widgets","parent":{"full_name":"acme/widgets"}}"#,
            ),
        );
        let git = git_syncing(r"^git fetch https://bitbucket\.org/acme/widgets\.git main$");
        let (ctx, bufs) = ctx_with(h, git, authed_config());

        run(&ctx, SyncArgs { source: None }).unwrap();
        assert!(bufs
            .stdout_string()
            .contains("✓ Synced me/widgets with acme/widgets (main)"));
    }

    #[test]
    fn sync_not_a_fork_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get fork",
            FakeTransport::rest(Method::Get, "/repositories/me/widgets"),
            FakeTransport::json(200, r#"{"full_name":"me/widgets"}"#),
        );
        let (ctx, _bufs) = ctx_with(h, git_no_calls(), authed_config());

        let err = run(&ctx, SyncArgs { source: None }).unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
    }

    #[test]
    fn sync_autodetect_not_authed_is_auth_error() {
        let h = Arc::new(FakeTransport::new());
        let (ctx, _bufs) = ctx_with(h, git_no_calls(), Arc::new(FileConfig::blank()));

        let err = run(&ctx, SyncArgs { source: None }).unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some(), "got: {err}");
    }
}
