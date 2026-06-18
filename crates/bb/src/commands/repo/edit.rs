//! `bb repo edit` and `bb repo rename` — update repository settings.

use crate::api::models::Repository;
use crate::api::BitbucketClient;
use crate::core::{AuthError, Context, FlagError};
use clap::Args;

#[derive(Args, Debug)]
pub struct EditArgs {
    /// New description
    #[arg(long)]
    pub description: Option<String>,
    /// Visibility
    #[arg(long, value_parser = ["public", "private"])]
    pub visibility: Option<String>,
    /// Move the repository under a project (by key)
    #[arg(long, value_name = "KEY")]
    pub project: Option<String>,
}

#[derive(Args, Debug)]
pub struct RenameArgs {
    /// New repository name
    #[arg(value_name = "NEW-NAME")]
    pub name: String,
}

#[derive(serde::Serialize)]
struct ProjectKey<'a> {
    key: &'a str,
}

#[derive(serde::Serialize)]
struct EditRepoBody<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    is_private: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    project: Option<ProjectKey<'a>>,
}

#[derive(serde::Serialize)]
struct RenameBody<'a> {
    name: &'a str,
}

/// Run `bb repo edit`.
///
/// # Errors
/// [`AuthError`] (4) unauthenticated; [`FlagError`] (1) when no field is given;
/// propagates [`ApiError`](crate::core::ApiError).
pub fn run(ctx: &Context, args: EditArgs) -> anyhow::Result<()> {
    if args.description.is_none() && args.visibility.is_none() && args.project.is_none() {
        return Err(FlagError::new(
            "nothing to update; pass --description, --visibility, or --project",
        )
        .into());
    }
    let (repo, client) = repo_and_client(ctx)?;

    let body = EditRepoBody {
        description: args.description.as_deref(),
        is_private: args.visibility.as_deref().map(|v| v == "private"),
        project: args.project.as_deref().map(|key| ProjectKey { key }),
    };
    let path = format!("/repositories/{}/{}", repo.workspace(), repo.slug());
    let _updated: Repository = client.put(&path, &body)?;
    ctx.io
        .println(&format!("✓ Updated {}/{}", repo.workspace(), repo.slug()));
    Ok(())
}

/// Run `bb repo rename`.
///
/// # Errors
/// [`AuthError`] (4) unauthenticated; propagates [`ApiError`](crate::core::ApiError).
pub fn run_rename(ctx: &Context, args: RenameArgs) -> anyhow::Result<()> {
    let (repo, client) = repo_and_client(ctx)?;
    let body = RenameBody { name: &args.name };
    let path = format!("/repositories/{}/{}", repo.workspace(), repo.slug());
    let _updated: Repository = client.put(&path, &body)?;
    ctx.io.println(&format!(
        "✓ Renamed {}/{} → {}",
        repo.workspace(),
        repo.slug(),
        args.name
    ));
    Ok(())
}

/// Resolve the current repo and an authenticated client.
fn repo_and_client(ctx: &Context) -> anyhow::Result<(crate::core::RepoId, BitbucketClient)> {
    let repo = ctx.base_repo()?;
    let host = repo.host().to_owned();
    let Some(header) = crate::auth::header_for(ctx.config.as_ref(), &host) else {
        return Err(AuthError::new(host).into());
    };
    let client = BitbucketClient::new(ctx.transport.clone(), Some(header));
    Ok((repo, client))
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

    fn stub_put(h: &Arc<FakeTransport>) {
        h.stub(
            "put repo",
            FakeTransport::rest(Method::Put, "/repositories/acme/widgets"),
            FakeTransport::json(200, r#"{"slug":"widgets","full_name":"acme/widgets"}"#),
        );
    }

    fn put_body(h: &FakeTransport) -> serde_json::Value {
        let reqs = h.requests.lock().unwrap();
        let put = reqs.iter().find(|r| r.method == Method::Put).unwrap();
        serde_json::from_slice(put.body.as_deref().unwrap()).unwrap()
    }

    fn edit_args() -> EditArgs {
        EditArgs {
            description: None,
            visibility: None,
            project: None,
        }
    }

    #[test]
    fn edit_updates_description() {
        let h = Arc::new(FakeTransport::new());
        stub_put(&h);
        let (ctx, bufs) = ctx_with(h.clone(), authed_config());

        let a = EditArgs {
            description: Some("new desc".to_owned()),
            ..edit_args()
        };
        run(&ctx, a).unwrap();
        assert_eq!(put_body(&h)["description"], "new desc");
        assert!(bufs.stdout_string().contains("✓ Updated acme/widgets"));
    }

    #[test]
    fn edit_visibility_private_and_public() {
        let h = Arc::new(FakeTransport::new());
        stub_put(&h);
        let (ctx, _bufs) = ctx_with(h.clone(), authed_config());
        run(
            &ctx,
            EditArgs {
                visibility: Some("private".to_owned()),
                ..edit_args()
            },
        )
        .unwrap();
        assert_eq!(put_body(&h)["is_private"], true);

        let h2 = Arc::new(FakeTransport::new());
        stub_put(&h2);
        let (ctx2, _b2) = ctx_with(h2.clone(), authed_config());
        run(
            &ctx2,
            EditArgs {
                visibility: Some("public".to_owned()),
                ..edit_args()
            },
        )
        .unwrap();
        assert_eq!(put_body(&h2)["is_private"], false);
    }

    #[test]
    fn edit_no_fields_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        let (ctx, _bufs) = ctx_with(h.clone(), authed_config());
        let err = run(&ctx, edit_args()).unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
        assert_eq!(h.request_count(), 0);
    }

    #[test]
    fn rename_puts_name() {
        let h = Arc::new(FakeTransport::new());
        stub_put(&h);
        let (ctx, bufs) = ctx_with(h.clone(), authed_config());
        run_rename(
            &ctx,
            RenameArgs {
                name: "widgets-2".to_owned(),
            },
        )
        .unwrap();
        assert_eq!(put_body(&h)["name"], "widgets-2");
        assert!(bufs
            .stdout_string()
            .contains("✓ Renamed acme/widgets → widgets-2"));
    }

    #[test]
    fn edit_not_authed_is_auth_error() {
        let h = Arc::new(FakeTransport::new());
        let (ctx, _bufs) = ctx_with(h.clone(), Arc::new(FileConfig::blank()));
        let a = EditArgs {
            description: Some("x".to_owned()),
            ..edit_args()
        };
        let err = run(&ctx, a).unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some(), "got: {err}");
    }
}
