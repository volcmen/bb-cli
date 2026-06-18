//! `bb pipeline run` / `stop` — trigger and stop pipelines.

use crate::api::models::Pipeline;
use crate::api::BitbucketClient;
use crate::core::{AuthError, Context, Method, RepoId};
use clap::Args;

#[derive(Args, Debug)]
pub struct RunArgs {
    /// Branch to run the pipeline for (default: current branch)
    #[arg(long)]
    pub branch: Option<String>,
    /// Run a custom pipeline by name
    #[arg(long, value_name = "NAME")]
    pub custom: Option<String>,
}

#[derive(Args, Debug)]
pub struct StopArgs {
    /// Pipeline build number
    #[arg(value_name = "BUILD")]
    pub id: String,
}

#[derive(serde::Serialize)]
struct Selector<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
    pattern: &'a str,
}

#[derive(serde::Serialize)]
struct Target<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
    ref_type: &'a str,
    ref_name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    selector: Option<Selector<'a>>,
}

#[derive(serde::Serialize)]
struct RunBody<'a> {
    target: Target<'a>,
}

/// Run `bb pipeline run`.
///
/// # Errors
/// [`AuthError`] (4) unauthenticated; propagates git/API errors.
pub fn run(ctx: &Context, args: RunArgs) -> anyhow::Result<()> {
    let (repo, client) = repo_and_client(ctx)?;
    let branch = match &args.branch {
        Some(b) => b.clone(),
        None => ctx.git.current_branch()?,
    };
    let body = RunBody {
        target: Target {
            kind: "pipeline_ref_target",
            ref_type: "branch",
            ref_name: &branch,
            selector: args.custom.as_deref().map(|pattern| Selector {
                kind: "custom",
                pattern,
            }),
        },
    };
    let path = format!(
        "/repositories/{}/{}/pipelines/",
        repo.workspace(),
        repo.slug()
    );
    let pipeline: Pipeline = client.post(&path, &body)?;
    let build = pipeline
        .build_number
        .map_or_else(|| "?".to_owned(), |n| n.to_string());
    ctx.io.println(&format!("✓ Started pipeline #{build}"));
    ctx.io.println(&format!(
        "https://bitbucket.org/{}/{}/pipelines/results/{build}",
        repo.workspace(),
        repo.slug()
    ));
    Ok(())
}

/// Run `bb pipeline stop`.
///
/// # Errors
/// [`AuthError`] (4) unauthenticated; propagates API errors.
pub fn run_stop(ctx: &Context, args: StopArgs) -> anyhow::Result<()> {
    let (repo, client) = repo_and_client(ctx)?;
    let path = format!(
        "/repositories/{}/{}/pipelines/{}/stopPipeline",
        repo.workspace(),
        repo.slug(),
        args.id
    );
    client.send_empty(Method::Post, &path)?;
    ctx.io.println(&format!("✓ Stopped pipeline #{}", args.id));
    Ok(())
}

fn repo_and_client(ctx: &Context) -> anyhow::Result<(RepoId, BitbucketClient)> {
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
    use crate::core::{ConfigProvider, GitClient, Transport};
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

    /// Git stub whose current branch is `feature`.
    fn git_on_feature() -> Arc<dyn GitClient> {
        let s = Arc::new(StubRunner::new());
        s.register(r"rev-parse --abbrev-ref HEAD", 0, "feature\n");
        Arc::new(ShellGit::new(s))
    }

    fn git_plain() -> Arc<dyn GitClient> {
        Arc::new(ShellGit::new(Arc::new(StubRunner::new())))
    }

    fn ctx_with(
        http: Arc<FakeTransport>,
        config: Arc<dyn ConfigProvider>,
        git: Arc<dyn GitClient>,
    ) -> (Context, crate::core::TestBuffers) {
        let transport: Arc<dyn Transport> = http;
        let (mut ctx, bufs) = test_context(
            transport,
            git,
            config,
            Arc::new(ScriptedPrompter::new()),
            false,
        );
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));
        (ctx, bufs)
    }

    fn stub_run(h: &Arc<FakeTransport>) {
        h.stub(
            "run",
            FakeTransport::rest(Method::Post, "/repositories/acme/widgets/pipelines/"),
            FakeTransport::json(201, r#"{"build_number":12,"uuid":"{p1}"}"#),
        );
    }
    fn posted(h: &FakeTransport) -> serde_json::Value {
        let reqs = h.requests.lock().unwrap();
        let post = reqs.iter().find(|r| r.method == Method::Post).unwrap();
        serde_json::from_slice(post.body.as_deref().unwrap()).unwrap()
    }

    #[test]
    fn run_posts_branch_target() {
        let h = Arc::new(FakeTransport::new());
        stub_run(&h);
        let (ctx, bufs) = ctx_with(h.clone(), authed_config(), git_on_feature());
        run(
            &ctx,
            RunArgs {
                branch: None,
                custom: None,
            },
        )
        .unwrap();
        let body = posted(&h);
        assert_eq!(body["target"]["ref_name"], "feature");
        assert_eq!(body["target"]["ref_type"], "branch");
        assert!(bufs.stdout_string().contains("✓ Started pipeline #12"));
    }

    #[test]
    fn run_explicit_branch() {
        let h = Arc::new(FakeTransport::new());
        stub_run(&h);
        let (ctx, _bufs) = ctx_with(h.clone(), authed_config(), git_plain());
        run(
            &ctx,
            RunArgs {
                branch: Some("main".to_owned()),
                custom: None,
            },
        )
        .unwrap();
        assert_eq!(posted(&h)["target"]["ref_name"], "main");
    }

    #[test]
    fn run_with_custom_selector() {
        let h = Arc::new(FakeTransport::new());
        stub_run(&h);
        let (ctx, _bufs) = ctx_with(h.clone(), authed_config(), git_plain());
        run(
            &ctx,
            RunArgs {
                branch: Some("main".to_owned()),
                custom: Some("nightly".to_owned()),
            },
        )
        .unwrap();
        let body = posted(&h);
        assert_eq!(body["target"]["selector"]["type"], "custom");
        assert_eq!(body["target"]["selector"]["pattern"], "nightly");
    }

    #[test]
    fn stop_posts_stop_pipeline() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "stop",
            FakeTransport::rest(Method::Post, "/pipelines/12/stopPipeline"),
            FakeTransport::json(204, ""),
        );
        let (ctx, bufs) = ctx_with(h.clone(), authed_config(), git_plain());
        run_stop(
            &ctx,
            StopArgs {
                id: "12".to_owned(),
            },
        )
        .unwrap();
        assert!(bufs.stdout_string().contains("✓ Stopped pipeline #12"));
    }

    #[test]
    fn not_authed_is_auth_error() {
        let h = Arc::new(FakeTransport::new());
        let (ctx, _bufs) = ctx_with(h.clone(), Arc::new(FileConfig::blank()), git_plain());
        let err = run(
            &ctx,
            RunArgs {
                branch: Some("main".to_owned()),
                custom: None,
            },
        )
        .unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some(), "got: {err}");
    }
}
