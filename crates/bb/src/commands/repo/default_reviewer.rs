//! `bb repo default-reviewer list|add|remove` — repository default reviewers.
//!
//! Default reviewers are auto-added by `bb pr create`.

use crate::api::models::User;
use crate::api::BitbucketClient;
use crate::core::{AuthError, Context, Method};
use clap::{Args, Subcommand};

#[derive(Args, Debug)]
pub struct DefaultReviewerArgs {
    #[command(subcommand)]
    command: DefaultReviewerCommands,
}

#[derive(Subcommand, Debug)]
enum DefaultReviewerCommands {
    /// List default reviewers
    List(ListArgs),
    /// Add a default reviewer
    Add(TargetArgs),
    /// Remove a default reviewer
    Remove(TargetArgs),
}

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Maximum number to list
    #[arg(long, default_value_t = 30)]
    pub limit: usize,
}

#[derive(Args, Debug)]
pub struct TargetArgs {
    /// The user (username or uuid)
    #[arg(value_name = "USER")]
    pub user: String,
}

/// Dispatch `bb repo default-reviewer <sub>`.
///
/// # Errors
/// Propagates the sub-command's error.
pub fn run(ctx: &Context, args: DefaultReviewerArgs) -> anyhow::Result<()> {
    match args.command {
        DefaultReviewerCommands::List(a) => list(ctx, a),
        DefaultReviewerCommands::Add(a) => add(ctx, a),
        DefaultReviewerCommands::Remove(a) => remove(ctx, a),
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
    let users: Vec<User> =
        client.paginate(&format!("{base}/default-reviewers"), Some(args.limit))?;
    if users.is_empty() {
        ctx.io.println("No default reviewers set");
        return Ok(());
    }
    for u in &users {
        let handle = u.username.as_deref().unwrap_or("");
        ctx.io.println(&format!("{}\t{handle}", u.label()));
    }
    Ok(())
}

fn add(ctx: &Context, args: TargetArgs) -> anyhow::Result<()> {
    let (client, base) = client_and_base(ctx)?;
    client.send_empty(
        Method::Put,
        &format!("{base}/default-reviewers/{}", args.user),
    )?;
    ctx.io
        .println(&format!("✓ Added default reviewer {}", args.user));
    Ok(())
}

fn remove(ctx: &Context, args: TargetArgs) -> anyhow::Result<()> {
    let (client, base) = client_and_base(ctx)?;
    client.send_empty(
        Method::Delete,
        &format!("{base}/default-reviewers/{}", args.user),
    )?;
    ctx.io
        .println(&format!("✓ Removed default reviewer {}", args.user));
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
    fn list_renders_users() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list",
            FakeTransport::rest(Method::Get, "/repositories/acme/widgets/default-reviewers"),
            FakeTransport::json(
                200,
                r#"{"values":[{"display_name":"Alice","username":"alice"}]}"#,
            ),
        );
        let (ctx, bufs) = ctx_with(h, authed());
        list(&ctx, ListArgs { limit: 30 }).unwrap();
        assert!(bufs.stdout_string().contains("Alice\talice"));
    }

    #[test]
    fn add_puts_user_path() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "add",
            FakeTransport::rest(
                Method::Put,
                "/repositories/acme/widgets/default-reviewers/alice",
            ),
            FakeTransport::json(200, r#"{"username":"alice"}"#),
        );
        let (ctx, bufs) = ctx_with(h, authed());
        add(
            &ctx,
            TargetArgs {
                user: "alice".to_owned(),
            },
        )
        .unwrap();
        assert!(bufs
            .stdout_string()
            .contains("✓ Added default reviewer alice"));
    }

    #[test]
    fn remove_deletes_user_path() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "remove",
            FakeTransport::rest(
                Method::Delete,
                "/repositories/acme/widgets/default-reviewers/bob",
            ),
            FakeTransport::json(204, ""),
        );
        let (ctx, bufs) = ctx_with(h, authed());
        remove(
            &ctx,
            TargetArgs {
                user: "bob".to_owned(),
            },
        )
        .unwrap();
        assert!(bufs
            .stdout_string()
            .contains("✓ Removed default reviewer bob"));
    }

    #[test]
    fn list_not_authed_is_auth_error() {
        let h = Arc::new(FakeTransport::new());
        let (ctx, _bufs) = ctx_with(h, Arc::new(FileConfig::blank()));
        let err = list(&ctx, ListArgs { limit: 30 }).unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some(), "got: {err}");
    }
}
