//! `bb pr diff` — print a pull request's diff (raw, pipe-friendly: no color).

use crate::api::BitbucketClient;
use crate::core::{AuthError, Context};
use clap::Args;

use super::finder;
use crate::auth;

#[derive(Args, Debug)]
pub struct DiffArgs {
    /// Pull request id (defaults to the PR for the current branch)
    #[arg(value_name = "ID")]
    pub id: Option<String>,
}

/// Run `bb pr diff`.
///
/// # Errors
/// Returns [`AuthError`] (exit 4) if not authenticated for the repo's host,
/// [`FlagError`](crate::core::FlagError) for a malformed id, and propagates
/// [`ApiError`](crate::core::ApiError) from the lookup or diff fetch.
pub fn run(ctx: &Context, args: DiffArgs) -> anyhow::Result<()> {
    let repo = ctx.base_repo()?;
    let host = repo.host().to_owned();

    let Some(header) = auth::header_for(ctx.config.as_ref(), &host) else {
        return Err(AuthError::new(host).into());
    };
    let client = BitbucketClient::new(ctx.transport.clone(), Some(header));

    // Resolve the id: parse a given selector, else infer from the current branch.
    let id = match args.id.as_deref() {
        Some(sel) => finder::parse_id(sel)?,
        None => finder::resolve(ctx, &client, &repo, None)?.id,
    };

    let path = format!(
        "/repositories/{}/{}/pullrequests/{id}/diff",
        repo.workspace(),
        repo.slug()
    );
    let diff = client.get_raw(&path)?;
    ctx.io.print(&diff);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::api::testing::FakeTransport;
    use crate::config::FileConfig;
    use crate::core::{ConfigProvider, GitClient, Method, Transport};
    use crate::git::{ShellGit, StubRunner};

    use super::*;
    use crate::testsupport::{test_context, ScriptedPrompter};

    fn git() -> Arc<dyn GitClient> {
        let s = Arc::new(StubRunner::new());
        s.register(
            "remote -v",
            0,
            "origin\tgit@bitbucket.org:acme/widgets.git (fetch)\n\
             origin\tgit@bitbucket.org:acme/widgets.git (push)\n",
        );
        Arc::new(ShellGit::new(s))
    }

    fn git_with_branch(branch: &str) -> Arc<dyn GitClient> {
        let s = Arc::new(StubRunner::new());
        s.register(
            "remote -v",
            0,
            "origin\tgit@bitbucket.org:acme/widgets.git (fetch)\n\
             origin\tgit@bitbucket.org:acme/widgets.git (push)\n",
        );
        s.register("rev-parse --abbrev-ref HEAD", 0, &format!("{branch}\n"));
        Arc::new(ShellGit::new(s))
    }

    fn config() -> Arc<dyn ConfigProvider> {
        let cfg = FileConfig::blank();
        cfg.set("bitbucket.org", "auth_type", "app_password")
            .unwrap();
        cfg.set("bitbucket.org", "username", "u").unwrap();
        cfg.set("bitbucket.org", "token", "t").unwrap();
        Arc::new(cfg)
    }

    fn args(id: Option<&str>) -> DiffArgs {
        DiffArgs {
            id: id.map(ToOwned::to_owned),
        }
    }

    const SAMPLE_DIFF: &str = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-old\n+new\n";

    #[test]
    fn diff_by_id_prints_raw() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "diff 42",
            FakeTransport::rest(Method::Get, "/pullrequests/42/diff"),
            FakeTransport::text(200, SAMPLE_DIFF),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, bufs) = test_context(transport, git(), config(), prompter, false);

        run(&ctx, args(Some("42"))).unwrap();
        assert_eq!(bufs.stdout_string(), SAMPLE_DIFF);
    }

    #[test]
    fn diff_does_not_fetch_the_pr_when_id_given() {
        // Only the diff endpoint is stubbed; if `run` tried to fetch the PR JSON
        // first, FakeTransport would panic on an unstubbed request.
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "diff 42",
            FakeTransport::rest(Method::Get, "/pullrequests/42/diff"),
            FakeTransport::text(200, SAMPLE_DIFF),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, _bufs) = test_context(transport, git(), config(), prompter, false);

        run(&ctx, args(Some("42"))).unwrap();
        assert_eq!(h.request_count(), 1);
    }

    #[test]
    fn diff_resolves_by_current_branch() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list by branch",
            FakeTransport::rest(Method::Get, "/pullrequests?state=OPEN&q="),
            FakeTransport::json(
                200,
                r#"{"values":[{"id":7,"title":"T","state":"OPEN",
                    "source":{"branch":{"name":"feature/x"}},
                    "destination":{"branch":{"name":"main"}}}]}"#,
            ),
        );
        h.stub(
            "diff 7",
            FakeTransport::rest(Method::Get, "/pullrequests/7/diff"),
            FakeTransport::text(200, SAMPLE_DIFF),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, bufs) = test_context(
            transport,
            git_with_branch("feature/x"),
            config(),
            prompter,
            false,
        );

        run(&ctx, args(None)).unwrap();
        assert_eq!(bufs.stdout_string(), SAMPLE_DIFF);
    }

    #[test]
    fn diff_not_logged_in_is_auth_error() {
        let h = Arc::new(FakeTransport::new());
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let cfg: Arc<dyn ConfigProvider> = Arc::new(FileConfig::blank());
        let (ctx, _bufs) = test_context(transport, git(), cfg, prompter, false);

        let err = run(&ctx, args(Some("42"))).unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some());
    }

    #[test]
    fn diff_invalid_id_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, _bufs) = test_context(transport, git(), config(), prompter, false);

        let err = run(&ctx, args(Some("nope"))).unwrap_err();
        assert!(err.downcast_ref::<crate::core::FlagError>().is_some());
    }
}
