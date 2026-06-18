//! `bb repo branch-restriction list|create|delete` — branch protection rules.

use crate::api::models::BranchRestriction;
use crate::api::BitbucketClient;
use crate::core::{AuthError, Context, Method};
use clap::{Args, Subcommand};

#[derive(Args, Debug)]
pub struct BranchRestrictionArgs {
    #[command(subcommand)]
    command: BranchRestrictionCommands,
}

#[derive(Subcommand, Debug)]
enum BranchRestrictionCommands {
    /// List branch restrictions
    List(ListArgs),
    /// Create a branch restriction
    Create(CreateArgs),
    /// Delete a branch restriction by id
    Delete(DeleteArgs),
}

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Maximum number to list
    #[arg(long, default_value_t = 30)]
    pub limit: usize,
}

#[derive(Args, Debug)]
pub struct CreateArgs {
    /// The restriction kind (e.g. push, force, delete, require_approvals_to_merge)
    #[arg(long)]
    pub kind: String,
    /// The branch glob pattern (e.g. main, release/*)
    #[arg(long)]
    pub pattern: String,
}

#[derive(Args, Debug)]
pub struct DeleteArgs {
    /// The branch restriction id
    #[arg(value_name = "ID")]
    pub id: i64,
}

#[derive(serde::Serialize)]
struct CreateBody<'a> {
    kind: &'a str,
    branch_match_kind: &'a str,
    pattern: &'a str,
}

/// Dispatch `bb repo branch-restriction <sub>`.
///
/// # Errors
/// Propagates the sub-command's error.
pub fn run(ctx: &Context, args: BranchRestrictionArgs) -> anyhow::Result<()> {
    match args.command {
        BranchRestrictionCommands::List(a) => list(ctx, a),
        BranchRestrictionCommands::Create(a) => create(ctx, a),
        BranchRestrictionCommands::Delete(a) => delete(ctx, a),
    }
}

fn client_and_base(ctx: &Context) -> anyhow::Result<(BitbucketClient, String)> {
    let repo = ctx.base_repo()?;
    let host = repo.host().to_owned();
    let Some(header) = crate::auth::header_for(ctx.config.as_ref(), &host) else {
        return Err(AuthError::new(host).into());
    };
    let client = BitbucketClient::new(ctx.transport.clone(), Some(header));
    let base = format!("/repositories/{}/{}", repo.workspace(), repo.slug());
    Ok((client, base))
}

fn list(ctx: &Context, args: ListArgs) -> anyhow::Result<()> {
    let (client, base) = client_and_base(ctx)?;
    let rules: Vec<BranchRestriction> =
        client.paginate(&format!("{base}/branch-restrictions"), Some(args.limit))?;
    if rules.is_empty() {
        ctx.io.println("No branch restrictions found");
        return Ok(());
    }
    for r in &rules {
        let id =
            r.id.map(|i| i.to_string())
                .unwrap_or_else(|| "?".to_owned());
        let kind = r.kind.as_deref().unwrap_or("");
        let pattern = r.pattern.as_deref().unwrap_or("");
        ctx.io.println(&format!("{id}\t{kind}\t{pattern}"));
    }
    Ok(())
}

fn create(ctx: &Context, args: CreateArgs) -> anyhow::Result<()> {
    let (client, base) = client_and_base(ctx)?;
    let body = CreateBody {
        kind: &args.kind,
        branch_match_kind: "glob",
        pattern: &args.pattern,
    };
    let created: BranchRestriction = client.post(&format!("{base}/branch-restrictions"), &body)?;
    ctx.io.println(&format!(
        "✓ Created branch restriction {} ({} on {})",
        created
            .id
            .map(|i| i.to_string())
            .unwrap_or_else(|| "?".to_owned()),
        args.kind,
        args.pattern
    ));
    Ok(())
}

fn delete(ctx: &Context, args: DeleteArgs) -> anyhow::Result<()> {
    let (client, base) = client_and_base(ctx)?;
    client.send_empty(
        Method::Delete,
        &format!("{base}/branch-restrictions/{}", args.id),
    )?;
    ctx.io
        .println(&format!("✓ Deleted branch restriction {}", args.id));
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

    fn authed() -> Arc<dyn ConfigProvider> {
        let cfg = FileConfig::blank();
        cfg.set(HOST, "auth_type", "app_password").unwrap();
        cfg.set(HOST, "username", "u").unwrap();
        cfg.set(HOST, "token", "t").unwrap();
        Arc::new(cfg)
    }

    fn ctx_with(
        http: Arc<FakeTransport>,
        cfg: Arc<dyn ConfigProvider>,
    ) -> (Context, crate::core::TestBuffers) {
        let transport: Arc<dyn Transport> = http;
        let git: Arc<dyn GitClient> = Arc::new(ShellGit::new(Arc::new(StubRunner::new())));
        let (mut ctx, bufs) = test_context(
            transport,
            git,
            cfg,
            Arc::new(ScriptedPrompter::new()),
            false,
        );
        ctx.repo_override = Some(RepoId::new("acme", "widgets"));
        (ctx, bufs)
    }

    #[test]
    fn list_renders_rules() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list",
            FakeTransport::rest(
                Method::Get,
                "/repositories/acme/widgets/branch-restrictions",
            ),
            FakeTransport::json(
                200,
                r#"{"values":[{"id":11,"kind":"push","pattern":"main"}]}"#,
            ),
        );
        let (ctx, bufs) = ctx_with(h, authed());
        list(&ctx, ListArgs { limit: 30 }).unwrap();
        assert!(bufs.stdout_string().contains("11\tpush\tmain"));
    }

    #[test]
    fn create_posts_body() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "create",
            FakeTransport::rest(
                Method::Post,
                "/repositories/acme/widgets/branch-restrictions",
            ),
            FakeTransport::json(201, r#"{"id":12,"kind":"push","pattern":"main"}"#),
        );
        let (ctx, bufs) = ctx_with(h.clone(), authed());
        create(
            &ctx,
            CreateArgs {
                kind: "push".to_owned(),
                pattern: "main".to_owned(),
            },
        )
        .unwrap();
        let reqs = h.requests.lock().unwrap();
        let post = reqs.iter().find(|r| r.method == Method::Post).unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(post.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["kind"], "push");
        assert_eq!(body["branch_match_kind"], "glob");
        assert_eq!(body["pattern"], "main");
        assert!(bufs
            .stdout_string()
            .contains("✓ Created branch restriction 12"));
    }

    #[test]
    fn delete_hits_id_path() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "delete",
            FakeTransport::rest(
                Method::Delete,
                "/repositories/acme/widgets/branch-restrictions/9",
            ),
            FakeTransport::json(204, ""),
        );
        let (ctx, bufs) = ctx_with(h, authed());
        delete(&ctx, DeleteArgs { id: 9 }).unwrap();
        assert!(bufs
            .stdout_string()
            .contains("✓ Deleted branch restriction 9"));
    }

    #[test]
    fn list_not_authed_is_auth_error() {
        let h = Arc::new(FakeTransport::new());
        let (ctx, _bufs) = ctx_with(h, Arc::new(FileConfig::blank()));
        let err = list(&ctx, ListArgs { limit: 30 }).unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some(), "got: {err}");
    }
}
