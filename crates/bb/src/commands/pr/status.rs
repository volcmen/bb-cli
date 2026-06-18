//! `bb pr status` — pull requests relevant to you in the current repo.

use crate::api::models::{PullRequest, User};
use crate::api::BitbucketClient;
use crate::core::{AuthError, Context};
use clap::Args;

use crate::render::{percent_encode, sanitize};

#[derive(Args, Debug)]
pub struct StatusArgs {}

/// Run `bb pr status`.
///
/// # Errors
/// Returns [`AuthError`] (exit 4) when not authenticated, and propagates
/// [`ApiError`](crate::core::ApiError).
pub fn run(ctx: &Context, _args: StatusArgs) -> anyhow::Result<()> {
    let repo = ctx.base_repo()?;
    let host = repo.host().to_owned();
    let Some(header) = crate::auth::header_for(ctx.config.as_ref(), &host) else {
        return Err(AuthError::new(host).into());
    };
    let client = BitbucketClient::new(ctx.transport.clone(), Some(header));

    let me: User = client.get("/user")?;
    let uuid = me.uuid.unwrap_or_default();

    // Created by you (issued first), then requesting your review.
    let authored = query(&client, &repo, &format!("author.uuid=\"{uuid}\""))?;
    let reviewing = query(&client, &repo, &format!("reviewers.uuid=\"{uuid}\""))?;

    print_section(ctx, "Created by you", &authored);
    ctx.io.println("");
    print_section(ctx, "Requesting your review", &reviewing);
    Ok(())
}

/// Fetch open PRs matching `clause`, folding the state into the BBQL `q` (#114).
fn query(
    client: &BitbucketClient,
    repo: &crate::core::RepoId,
    clause: &str,
) -> anyhow::Result<Vec<PullRequest>> {
    let q = percent_encode(&format!("state=\"OPEN\" AND {clause}"));
    let path = format!(
        "/repositories/{}/{}/pullrequests?pagelen=50&q={q}",
        repo.workspace(),
        repo.slug()
    );
    Ok(client.paginate(&path, None)?)
}

fn print_section(ctx: &Context, heading: &str, prs: &[PullRequest]) {
    ctx.io.println(heading);
    if prs.is_empty() {
        ctx.io.println("  (none)");
        return;
    }
    for pr in prs {
        let title = sanitize(pr.title.as_deref().unwrap_or_default());
        let src = pr.source.branch.as_ref().map_or("?", |b| b.name.as_str());
        let dst = pr
            .destination
            .branch
            .as_ref()
            .map_or("?", |b| b.name.as_str());
        ctx.io
            .println(&format!("  #{}  {title}  ({src} → {dst})", pr.id));
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

    fn git() -> Arc<dyn GitClient> {
        Arc::new(ShellGit::new(Arc::new(StubRunner::new())))
    }

    fn ctx_with(
        http: Arc<FakeTransport>,
        config: Arc<dyn ConfigProvider>,
    ) -> (Context, crate::core::TestBuffers) {
        let transport: Arc<dyn Transport> = http;
        let (mut ctx, bufs) = test_context(
            transport,
            git(),
            config,
            Arc::new(ScriptedPrompter::new()),
            false,
        );
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));
        (ctx, bufs)
    }

    fn stub_user(h: &Arc<FakeTransport>) {
        h.stub(
            "user",
            FakeTransport::rest(Method::Get, "/2.0/user"),
            FakeTransport::json(200, r#"{"uuid":"{abc}","display_name":"Me"}"#),
        );
    }

    #[test]
    fn status_lists_authored_and_review_sections() {
        let h = Arc::new(FakeTransport::new());
        stub_user(&h);
        // First /pullrequests query = authored.
        h.stub(
            "authored",
            FakeTransport::rest(Method::Get, "/pullrequests"),
            FakeTransport::json(
                200,
                r#"{"values":[{"id":7,"title":"My PR",
                    "source":{"branch":{"name":"feat"}},
                    "destination":{"branch":{"name":"main"}}}]}"#,
            ),
        );
        // Second /pullrequests query = review-requested.
        h.stub(
            "reviewing",
            FakeTransport::rest(Method::Get, "/pullrequests"),
            FakeTransport::json(
                200,
                r#"{"values":[{"id":9,"title":"Their PR",
                    "source":{"branch":{"name":"fix"}},
                    "destination":{"branch":{"name":"main"}}}]}"#,
            ),
        );
        let (ctx, bufs) = ctx_with(h.clone(), authed_config());

        run(&ctx, StatusArgs {}).unwrap();

        let out = bufs.stdout_string();
        assert!(out.contains("Created by you"), "out: {out}");
        assert!(out.contains("#7  My PR  (feat → main)"), "out: {out}");
        assert!(out.contains("Requesting your review"), "out: {out}");
        assert!(out.contains("#9  Their PR  (fix → main)"), "out: {out}");
    }

    #[test]
    fn status_empty_sections_show_none() {
        let h = Arc::new(FakeTransport::new());
        stub_user(&h);
        h.stub(
            "authored",
            FakeTransport::rest(Method::Get, "/pullrequests"),
            FakeTransport::json(200, r#"{"values":[]}"#),
        );
        h.stub(
            "reviewing",
            FakeTransport::rest(Method::Get, "/pullrequests"),
            FakeTransport::json(200, r#"{"values":[]}"#),
        );
        let (ctx, bufs) = ctx_with(h.clone(), authed_config());

        run(&ctx, StatusArgs {}).unwrap();

        let out = bufs.stdout_string();
        assert_eq!(out.matches("(none)").count(), 2, "out: {out}");
    }

    #[test]
    fn status_not_authed_is_auth_error() {
        let h = Arc::new(FakeTransport::new());
        let (ctx, _bufs) = ctx_with(h.clone(), Arc::new(FileConfig::blank()));
        let err = run(&ctx, StatusArgs {}).unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some(), "got: {err}");
    }
}
