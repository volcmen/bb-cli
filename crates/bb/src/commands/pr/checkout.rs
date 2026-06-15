//! `bb pr checkout` — check out a pull request's source branch locally.

use bb_core::{AuthError, Context};
use clap::Args;

use crate::auth;

#[derive(Args, Debug)]
pub struct CheckoutArgs {
    /// Pull request id
    #[arg(value_name = "ID")]
    pub id: String,
}

/// Run `bb pr checkout`.
///
/// Resolves the PR by id, fetches its source branch from `origin`, and checks
/// it out locally.
///
/// # Errors
/// Returns [`FlagError`](bb_core::FlagError) for a malformed id, [`AuthError`]
/// (exit 4) if not authenticated for the repo's host, an error if the PR is not
/// found, has no source branch, or its source lives in a fork, and propagates
/// any git failure.
//
// TODO(cross-fork #25): cross-fork checkout is still unsupported, but no longer
// silent — the guard below detects a fork source (via
// `pr.source.repo_full_name()`) and errors clearly instead of fetching the
// wrong branch from `origin`. To support it we must add the fork as a remote
// (`ctx.git.add_remote`) and fetch the branch from there.
pub fn run(ctx: &Context, args: CheckoutArgs) -> anyhow::Result<()> {
    let repo = ctx.base_repo()?;
    let host = repo.host().to_owned();

    let header = auth::header_for(ctx.config.as_ref(), &host);
    if header.is_none() {
        return Err(AuthError::new(host).into());
    }
    let client = bb_api::BitbucketClient::new(ctx.transport.clone(), header);

    let id = super::finder::parse_id(&args.id)?;
    let pr = super::finder::find_by_id(&client, &repo, id)?;

    let branch = pr.source.branch_name();
    if branch.is_empty() {
        anyhow::bail!("pull request #{id} has no source branch");
    }

    // Cross-fork guard: only the same-repo fast path (`fetch origin <branch>`)
    // is supported. When the source repository is known and differs from the
    // base repo, fetching from `origin` would grab the wrong branch (or fail),
    // so error clearly instead.
    if let Some(source) = pr.source.repo_full_name() {
        if source != repo.full_name() {
            anyhow::bail!(
                "checking out a pull request from a fork ({source}) is not supported yet; \
                 check it out manually"
            );
        }
    }

    ctx.git.fetch("origin", branch)?;
    ctx.git.checkout(branch)?;

    ctx.io
        .println(&format!("✓ Checked out branch '{branch}' for PR #{id}"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bb_api::testing::FakeTransport;
    use bb_config::FileConfig;
    use bb_core::{ConfigProvider, FlagError, GitClient, Method, Transport};
    use bb_git::{ShellGit, StubRunner};

    use super::*;
    use crate::testsupport::{test_context, ScriptedPrompter};

    /// A git stub that answers `remote -v` (so `base_repo` resolves) plus any
    /// additional command patterns the test expects to be issued. StubRunner's
    /// `Drop` asserts every registered pattern was hit, so register exactly the
    /// git commands the run should produce.
    fn git_with(patterns: &[&str]) -> Arc<dyn GitClient> {
        let s = Arc::new(StubRunner::new());
        s.register(
            "remote -v",
            0,
            "origin\tgit@bitbucket.org:acme/widgets.git (fetch)\n\
             origin\tgit@bitbucket.org:acme/widgets.git (push)\n",
        );
        for p in patterns {
            s.register(p, 0, "");
        }
        Arc::new(ShellGit::new(s))
    }

    /// A git stub that only answers `remote -v` (no fetch/checkout expected).
    fn git_repo_only() -> Arc<dyn GitClient> {
        git_with(&[])
    }

    fn config() -> Arc<dyn ConfigProvider> {
        let cfg = FileConfig::blank();
        cfg.set("bitbucket.org", "auth_type", "app_password")
            .unwrap();
        cfg.set("bitbucket.org", "username", "davidd").unwrap();
        cfg.set("bitbucket.org", "token", "secret").unwrap();
        Arc::new(cfg)
    }

    fn pr_json(id: u64, branch: &str) -> String {
        format!(
            r#"{{"id":{id},"title":"T","state":"OPEN",
                 "source":{{"branch":{{"name":"{branch}"}}}},
                 "destination":{{"branch":{{"name":"main"}}}}}}"#
        )
    }

    #[test]
    fn checkout_fetches_and_checks_out_source_branch() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get pr 42",
            FakeTransport::rest(Method::Get, "/pullrequests/42"),
            FakeTransport::json(200, &pr_json(42, "feature/x")),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let git = git_with(&["fetch origin feature/x", "checkout feature/x"]);
        let (ctx, bufs) = test_context(
            transport,
            git,
            config(),
            Arc::new(ScriptedPrompter::new()),
            false,
        );

        run(
            &ctx,
            CheckoutArgs {
                id: "42".to_owned(),
            },
        )
        .unwrap();

        // The git stub's Drop asserts fetch + checkout were both issued.
        let out = bufs.stdout_string();
        assert!(
            out.contains("✓ Checked out branch 'feature/x' for PR #42"),
            "stdout: {out}"
        );
    }

    #[test]
    fn checkout_from_fork_errors_without_touching_git() {
        let h = Arc::new(FakeTransport::new());
        // Source lives in a fork ("someone/fork"), base repo is "acme/widgets".
        let pr = r#"{"id":42,"title":"T","state":"OPEN",
                     "source":{"branch":{"name":"feature/x"},
                               "repository":{"full_name":"someone/fork"}},
                     "destination":{"branch":{"name":"main"}}}"#;
        h.stub(
            "get pr 42 from fork",
            FakeTransport::rest(Method::Get, "/pullrequests/42"),
            FakeTransport::json(200, pr),
        );
        let transport: Arc<dyn Transport> = h.clone();
        // Only `remote -v` is registered: no fetch/checkout must be issued.
        let (ctx, _bufs) = test_context(
            transport,
            git_repo_only(),
            config(),
            Arc::new(ScriptedPrompter::new()),
            false,
        );

        let err = run(
            &ctx,
            CheckoutArgs {
                id: "42".to_owned(),
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("fork"), "err: {err}");
    }

    #[test]
    fn checkout_invalid_id_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        let transport: Arc<dyn Transport> = h.clone();
        let (ctx, _bufs) = test_context(
            transport,
            git_repo_only(),
            config(),
            Arc::new(ScriptedPrompter::new()),
            false,
        );

        let err = run(
            &ctx,
            CheckoutArgs {
                id: "not-a-number".to_owned(),
            },
        )
        .unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some());
    }

    #[test]
    fn checkout_not_logged_in_is_auth_error() {
        let h = Arc::new(FakeTransport::new());
        let transport: Arc<dyn Transport> = h.clone();
        let cfg: Arc<dyn ConfigProvider> = Arc::new(FileConfig::blank());
        let (ctx, _bufs) = test_context(
            transport,
            git_repo_only(),
            cfg,
            Arc::new(ScriptedPrompter::new()),
            false,
        );

        let err = run(
            &ctx,
            CheckoutArgs {
                id: "42".to_owned(),
            },
        )
        .unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some());
    }

    #[test]
    fn checkout_pr_not_found_errors() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get pr 99 missing",
            FakeTransport::rest(Method::Get, "/pullrequests/99"),
            FakeTransport::json(404, r#"{"type":"error","error":{"message":"Not found"}}"#),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let (ctx, _bufs) = test_context(
            transport,
            git_repo_only(),
            config(),
            Arc::new(ScriptedPrompter::new()),
            false,
        );

        let err = run(
            &ctx,
            CheckoutArgs {
                id: "99".to_owned(),
            },
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("no pull request #99"),
            "err: {err}"
        );
    }
}
