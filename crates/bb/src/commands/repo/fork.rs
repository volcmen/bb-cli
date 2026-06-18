//! `bb repo fork` — fork a Bitbucket repository.

use crate::api::models::Repository;
use crate::api::BitbucketClient;
use crate::core::{AuthError, Context, FlagError, RepoId};
use clap::Args;

#[derive(Args, Debug)]
pub struct ForkArgs {
    /// Source repository as WORKSPACE/SLUG (defaults to the current repo)
    #[arg(value_name = "SOURCE")]
    pub source: Option<String>,
    /// Target workspace for the fork (default: your own)
    #[arg(long, value_name = "WS")]
    pub workspace: Option<String>,
    /// Name (slug) for the fork
    #[arg(long, value_name = "SLUG")]
    pub name: Option<String>,
}

#[derive(serde::Serialize)]
struct WorkspaceSlug<'a> {
    slug: &'a str,
}

#[derive(serde::Serialize)]
struct ForkBody<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace: Option<WorkspaceSlug<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
}

/// Run `bb repo fork`.
///
/// # Errors
/// Returns [`AuthError`] (exit 4) when not authenticated, [`FlagError`] (exit 1)
/// for a malformed `SOURCE`, and propagates [`ApiError`](crate::core::ApiError).
pub fn run(ctx: &Context, args: ForkArgs) -> anyhow::Result<()> {
    let source: RepoId = match &args.source {
        Some(s) => s
            .parse()
            .map_err(|e| anyhow::Error::from(FlagError::new(e)))?,
        None => ctx.base_repo()?,
    };
    let host = source.host().to_owned();

    let Some(header) = crate::auth::header_for(ctx.config.as_ref(), &host) else {
        return Err(AuthError::new(host).into());
    };
    let client = BitbucketClient::new(ctx.transport.clone(), Some(header));

    let body = ForkBody {
        workspace: args.workspace.as_deref().map(|slug| WorkspaceSlug { slug }),
        name: args.name.as_deref(),
    };

    let path = format!(
        "/repositories/{}/{}/forks",
        source.workspace(),
        source.slug()
    );
    let fork: Repository = client.post(&path, &body)?;

    let fork_name = fork
        .full_name
        .clone()
        .unwrap_or_else(|| format!("{}/{}", source.workspace(), source.slug()));
    ctx.io.println(&format!(
        "✓ Forked {}/{} → {fork_name}",
        source.workspace(),
        source.slug()
    ));
    if let Some(url) = fork.html_url() {
        ctx.io.println(url);
    }
    Ok(())
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

    fn stub_fork(h: &Arc<FakeTransport>) {
        h.stub(
            "fork",
            FakeTransport::rest(Method::Post, "/repositories/acme/widgets/forks"),
            FakeTransport::json(
                201,
                r#"{"slug":"widgets","full_name":"me/widgets",
                    "links":{"html":{"href":"https://bitbucket.org/me/widgets"}}}"#,
            ),
        );
    }

    fn posted(h: &FakeTransport) -> serde_json::Value {
        let reqs = h.requests.lock().unwrap();
        let post = reqs.iter().find(|r| r.method == Method::Post).unwrap();
        serde_json::from_slice(post.body.as_deref().unwrap()).unwrap()
    }

    fn args() -> ForkArgs {
        ForkArgs {
            source: None,
            workspace: None,
            name: None,
        }
    }

    #[test]
    fn fork_current_repo_posts_to_forks() {
        let h = Arc::new(FakeTransport::new());
        stub_fork(&h);
        let (ctx, bufs) = ctx_with(h.clone(), authed_config());

        run(&ctx, args()).unwrap();

        assert!(bufs
            .stdout_string()
            .contains("✓ Forked acme/widgets → me/widgets"));
    }

    #[test]
    fn fork_with_workspace_and_name() {
        let h = Arc::new(FakeTransport::new());
        stub_fork(&h);
        let (ctx, _bufs) = ctx_with(h.clone(), authed_config());

        let a = ForkArgs {
            workspace: Some("myteam".to_owned()),
            name: Some("widgets-fork".to_owned()),
            ..args()
        };
        run(&ctx, a).unwrap();

        let body = posted(&h);
        assert_eq!(body["workspace"]["slug"], "myteam");
        assert_eq!(body["name"], "widgets-fork");
    }

    #[test]
    fn fork_explicit_source() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "fork other",
            FakeTransport::rest(Method::Post, "/repositories/other/proj/forks"),
            FakeTransport::json(201, r#"{"full_name":"me/proj"}"#),
        );
        let (ctx, bufs) = ctx_with(h.clone(), authed_config());

        let a = ForkArgs {
            source: Some("other/proj".to_owned()),
            ..args()
        };
        run(&ctx, a).unwrap();
        assert!(bufs
            .stdout_string()
            .contains("✓ Forked other/proj → me/proj"));
    }

    #[test]
    fn fork_not_authed_is_auth_error() {
        let h = Arc::new(FakeTransport::new());
        let (ctx, _bufs) = ctx_with(h.clone(), Arc::new(FileConfig::blank()));
        let err = run(&ctx, args()).unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some(), "got: {err}");
    }
}
