//! `bb repo webhook list|create|delete` — repository webhooks.

use crate::api::models::Webhook;
use crate::api::BitbucketClient;
use crate::core::{AuthError, Context, FlagError, Method};
use clap::{Args, Subcommand};

#[derive(Args, Debug)]
pub struct WebhookArgs {
    #[command(subcommand)]
    command: WebhookCommands,
}

#[derive(Subcommand, Debug)]
enum WebhookCommands {
    /// List webhooks
    List(ListArgs),
    /// Create a webhook
    Create(CreateArgs),
    /// Delete a webhook by uuid
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
    /// The webhook endpoint URL
    #[arg(long)]
    pub url: String,
    /// Description for the webhook
    #[arg(long)]
    pub description: Option<String>,
    /// Event to subscribe to (repeatable; default: repo:push)
    #[arg(long = "event")]
    pub events: Vec<String>,
}

#[derive(Args, Debug)]
pub struct DeleteArgs {
    /// The webhook uuid
    #[arg(value_name = "UUID")]
    pub uuid: String,
}

#[derive(serde::Serialize)]
struct CreateBody<'a> {
    url: &'a str,
    description: &'a str,
    active: bool,
    events: Vec<String>,
}

/// Dispatch `bb repo webhook <sub>`.
///
/// # Errors
/// Propagates the sub-command's error.
pub fn run(ctx: &Context, args: WebhookArgs) -> anyhow::Result<()> {
    match args.command {
        WebhookCommands::List(a) => list(ctx, a),
        WebhookCommands::Create(a) => create(ctx, a),
        WebhookCommands::Delete(a) => delete(ctx, a),
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
    let hooks: Vec<Webhook> = client.paginate(&format!("{base}/hooks"), Some(args.limit))?;
    if hooks.is_empty() {
        ctx.io.println("No webhooks found");
        return Ok(());
    }
    for h in &hooks {
        let uuid = h.uuid.as_deref().unwrap_or("?");
        let url = h.url.as_deref().unwrap_or("");
        let events = h.events.join(",");
        ctx.io.println(&format!("{uuid}\t{url}\t{events}"));
    }
    Ok(())
}

fn create(ctx: &Context, args: CreateArgs) -> anyhow::Result<()> {
    let (client, base) = client_and_base(ctx)?;
    let events = if args.events.is_empty() {
        vec!["repo:push".to_owned()]
    } else {
        args.events.clone()
    };
    let body = CreateBody {
        url: &args.url,
        description: args.description.as_deref().unwrap_or("bb webhook"),
        active: true,
        events,
    };
    let hook: Webhook = client.post(&format!("{base}/hooks"), &body)?;
    ctx.io.println(&format!(
        "✓ Created webhook {}",
        hook.uuid.as_deref().unwrap_or(&args.url)
    ));
    Ok(())
}

fn delete(ctx: &Context, args: DeleteArgs) -> anyhow::Result<()> {
    if args.uuid.is_empty() {
        return Err(FlagError::new("a webhook uuid is required").into());
    }
    let (client, base) = client_and_base(ctx)?;
    client.send_empty(Method::Delete, &format!("{base}/hooks/{}", args.uuid))?;
    ctx.io.println(&format!("✓ Deleted webhook {}", args.uuid));
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
    fn list_renders_hooks() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list",
            FakeTransport::rest(Method::Get, "/repositories/acme/widgets/hooks"),
            FakeTransport::json(
                200,
                r#"{"values":[{"uuid":"{h1}","url":"https://ex.com/hook","events":["repo:push"]}]}"#,
            ),
        );
        let (ctx, bufs) = ctx_with(h, authed());
        list(&ctx, ListArgs { limit: 30 }).unwrap();
        assert!(bufs
            .stdout_string()
            .contains("{h1}\thttps://ex.com/hook\trepo:push"));
    }

    #[test]
    fn create_posts_body() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "create",
            FakeTransport::rest(Method::Post, "/repositories/acme/widgets/hooks"),
            FakeTransport::json(201, r#"{"uuid":"{new}"}"#),
        );
        let (ctx, bufs) = ctx_with(h.clone(), authed());
        create(
            &ctx,
            CreateArgs {
                url: "https://ex.com/hook".to_owned(),
                description: Some("CI".to_owned()),
                events: vec!["repo:push".to_owned(), "pullrequest:created".to_owned()],
            },
        )
        .unwrap();
        let reqs = h.requests.lock().unwrap();
        let post = reqs.iter().find(|r| r.method == Method::Post).unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(post.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["url"], "https://ex.com/hook");
        assert_eq!(body["events"][1], "pullrequest:created");
        assert!(bufs.stdout_string().contains("✓ Created webhook {new}"));
    }

    #[test]
    fn delete_hits_uuid_path() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "delete",
            FakeTransport::rest(Method::Delete, "/repositories/acme/widgets/hooks/{h1}"),
            FakeTransport::json(204, ""),
        );
        let (ctx, bufs) = ctx_with(h, authed());
        delete(
            &ctx,
            DeleteArgs {
                uuid: "{h1}".to_owned(),
            },
        )
        .unwrap();
        assert!(bufs.stdout_string().contains("✓ Deleted webhook {h1}"));
    }

    #[test]
    fn list_not_authed_is_auth_error() {
        let h = Arc::new(FakeTransport::new());
        let (ctx, _bufs) = ctx_with(h, Arc::new(FileConfig::blank()));
        let err = list(&ctx, ListArgs { limit: 30 }).unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some(), "got: {err}");
    }
}
