//! `bb repo create` — create a new Bitbucket repository.

use crate::api::models::Repository;
use crate::api::BitbucketClient;
use crate::core::{AuthError, Context, FlagError, RepoId};
use clap::Args;

#[derive(Args, Debug)]
pub struct CreateArgs {
    /// New repository as WORKSPACE/SLUG
    #[arg(value_name = "WORKSPACE/SLUG")]
    pub name: String,
    /// Make the repository public (default: private)
    #[arg(long)]
    pub public: bool,
    /// Repository description
    #[arg(long)]
    pub description: Option<String>,
    /// Project key to create the repository under
    #[arg(long, value_name = "KEY")]
    pub project: Option<String>,
}

#[derive(serde::Serialize)]
struct ProjectKey<'a> {
    key: &'a str,
}

#[derive(serde::Serialize)]
struct CreateRepoBody<'a> {
    scm: &'a str,
    is_private: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    project: Option<ProjectKey<'a>>,
}

/// Run `bb repo create`.
///
/// # Errors
/// Returns [`AuthError`] (exit 4) when not authenticated, [`FlagError`] (exit 1)
/// for a malformed `WORKSPACE/SLUG`, and propagates
/// [`ApiError`](crate::core::ApiError) (incl. a scope error if the consumer
/// lacks `repository:admin`).
pub fn run(ctx: &Context, args: CreateArgs) -> anyhow::Result<()> {
    let repo: RepoId = args
        .name
        .parse()
        .map_err(|e| anyhow::Error::from(FlagError::new(e)))?;
    let host = repo.host().to_owned();

    let Some(header) = crate::auth::header_for(ctx.config.as_ref(), &host) else {
        return Err(AuthError::new(host).into());
    };
    let client = BitbucketClient::new(ctx.transport.clone(), Some(header));

    let body = CreateRepoBody {
        scm: "git",
        is_private: !args.public,
        description: args.description.as_deref(),
        project: args.project.as_deref().map(|key| ProjectKey { key }),
    };

    let path = format!("/repositories/{}/{}", repo.workspace(), repo.slug());
    let created: Repository = client.post(&path, &body)?;

    ctx.io
        .println(&format!("✓ Created {}/{}", repo.workspace(), repo.slug()));
    let url = created.html_url().map_or_else(
        || format!("https://bitbucket.org/{}/{}", repo.workspace(), repo.slug()),
        ToOwned::to_owned,
    );
    ctx.io.println(&url);
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
        test_context(
            transport,
            git(),
            config,
            Arc::new(ScriptedPrompter::new()),
            false,
        )
    }

    fn args(name: &str) -> CreateArgs {
        CreateArgs {
            name: name.to_owned(),
            public: false,
            description: None,
            project: None,
        }
    }

    fn stub_create(h: &Arc<FakeTransport>) {
        h.stub(
            "create repo",
            FakeTransport::rest(Method::Post, "/repositories/acme/newrepo"),
            FakeTransport::json(
                201,
                r#"{"slug":"newrepo","full_name":"acme/newrepo","is_private":true,
                    "links":{"html":{"href":"https://bitbucket.org/acme/newrepo"}}}"#,
            ),
        );
    }

    fn posted(h: &FakeTransport) -> serde_json::Value {
        let reqs = h.requests.lock().unwrap();
        let post = reqs.iter().find(|r| r.method == Method::Post).unwrap();
        serde_json::from_slice(post.body.as_deref().unwrap()).unwrap()
    }

    #[test]
    fn create_private_by_default() {
        let h = Arc::new(FakeTransport::new());
        stub_create(&h);
        let (ctx, bufs) = ctx_with(h.clone(), authed_config());

        let a = CreateArgs {
            description: Some("a repo".to_owned()),
            ..args("acme/newrepo")
        };
        run(&ctx, a).unwrap();

        let body = posted(&h);
        assert_eq!(body["scm"], "git");
        assert_eq!(body["is_private"], true);
        assert_eq!(body["description"], "a repo");
        assert!(bufs.stdout_string().contains("✓ Created acme/newrepo"));
    }

    #[test]
    fn create_public_flag() {
        let h = Arc::new(FakeTransport::new());
        stub_create(&h);
        let (ctx, _bufs) = ctx_with(h.clone(), authed_config());

        let a = CreateArgs {
            public: true,
            ..args("acme/newrepo")
        };
        run(&ctx, a).unwrap();
        assert_eq!(posted(&h)["is_private"], false);
    }

    #[test]
    fn create_with_project() {
        let h = Arc::new(FakeTransport::new());
        stub_create(&h);
        let (ctx, _bufs) = ctx_with(h.clone(), authed_config());

        let a = CreateArgs {
            project: Some("PROJ".to_owned()),
            ..args("acme/newrepo")
        };
        run(&ctx, a).unwrap();
        assert_eq!(posted(&h)["project"]["key"], "PROJ");
    }

    #[test]
    fn create_invalid_name_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        let (ctx, _bufs) = ctx_with(h.clone(), authed_config());
        let err = run(&ctx, args("not-a-repo")).unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
        assert_eq!(h.request_count(), 0);
    }

    #[test]
    fn create_not_authed_is_auth_error() {
        let h = Arc::new(FakeTransport::new());
        let (ctx, _bufs) = ctx_with(h.clone(), Arc::new(FileConfig::blank()));
        let err = run(&ctx, args("acme/newrepo")).unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some(), "got: {err}");
    }
}
