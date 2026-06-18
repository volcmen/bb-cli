//! `bb repo deploy-key list|add|delete` — repository deploy keys.

use crate::api::models::DeployKey;
use crate::api::BitbucketClient;
use crate::core::{AuthError, Context, FlagError, Method};
use clap::{Args, Subcommand};

#[derive(Args, Debug)]
pub struct DeployKeyArgs {
    #[command(subcommand)]
    command: DeployKeyCommands,
}

#[derive(Subcommand, Debug)]
enum DeployKeyCommands {
    /// List deploy keys
    List(ListArgs),
    /// Add a deploy key
    Add(AddArgs),
    /// Delete a deploy key by id
    Delete(DeleteArgs),
}

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Maximum number to list
    #[arg(long, default_value_t = 30)]
    pub limit: usize,
}

#[derive(Args, Debug)]
pub struct AddArgs {
    /// The public key string (or use --key-file)
    #[arg(long)]
    pub key: Option<String>,
    /// Read the public key from a file
    #[arg(long = "key-file", value_name = "FILE")]
    pub key_file: Option<String>,
    /// A label for the key
    #[arg(long, short = 't')]
    pub title: Option<String>,
}

#[derive(Args, Debug)]
pub struct DeleteArgs {
    /// The deploy key id
    #[arg(value_name = "ID")]
    pub id: i64,
}

#[derive(serde::Serialize)]
struct AddBody<'a> {
    key: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<&'a str>,
}

/// Dispatch `bb repo deploy-key <sub>`.
///
/// # Errors
/// Propagates the sub-command's error.
pub fn run(ctx: &Context, args: DeployKeyArgs) -> anyhow::Result<()> {
    match args.command {
        DeployKeyCommands::List(a) => list(ctx, a),
        DeployKeyCommands::Add(a) => add(ctx, a),
        DeployKeyCommands::Delete(a) => delete(ctx, a),
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
    let keys: Vec<DeployKey> = client.paginate(&format!("{base}/deploy-keys"), Some(args.limit))?;
    if keys.is_empty() {
        ctx.io.println("No deploy keys found");
        return Ok(());
    }
    for k in &keys {
        let id =
            k.id.map(|i| i.to_string())
                .unwrap_or_else(|| "?".to_owned());
        let label = k.label.as_deref().unwrap_or("");
        ctx.io.println(&format!("{id}\t{label}"));
    }
    Ok(())
}

fn add(ctx: &Context, args: AddArgs) -> anyhow::Result<()> {
    let key = match (&args.key, &args.key_file) {
        (Some(k), _) => k.clone(),
        (None, Some(file)) => std::fs::read_to_string(file)?.trim().to_owned(),
        (None, None) => {
            return Err(FlagError::new("provide the key via --key or --key-file").into())
        }
    };
    let (client, base) = client_and_base(ctx)?;
    let body = AddBody {
        key: &key,
        label: args.title.as_deref(),
    };
    let added: DeployKey = client.post(&format!("{base}/deploy-keys"), &body)?;
    ctx.io.println(&format!(
        "✓ Added deploy key{}",
        added.id.map(|i| format!(" {i}")).unwrap_or_default()
    ));
    Ok(())
}

fn delete(ctx: &Context, args: DeleteArgs) -> anyhow::Result<()> {
    let (client, base) = client_and_base(ctx)?;
    client.send_empty(Method::Delete, &format!("{base}/deploy-keys/{}", args.id))?;
    ctx.io.println(&format!("✓ Deleted deploy key {}", args.id));
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
    fn list_renders_keys() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list",
            FakeTransport::rest(Method::Get, "/repositories/acme/widgets/deploy-keys"),
            FakeTransport::json(200, r#"{"values":[{"id":1,"label":"ci"}]}"#),
        );
        let (ctx, bufs) = ctx_with(h, authed());
        list(&ctx, ListArgs { limit: 30 }).unwrap();
        assert!(bufs.stdout_string().contains("1\tci"));
    }

    #[test]
    fn add_posts_key() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "add",
            FakeTransport::rest(Method::Post, "/repositories/acme/widgets/deploy-keys"),
            FakeTransport::json(201, r#"{"id":5,"label":"ci"}"#),
        );
        let (ctx, bufs) = ctx_with(h.clone(), authed());
        add(
            &ctx,
            AddArgs {
                key: Some("ssh-ed25519 AAAA...".to_owned()),
                key_file: None,
                title: Some("ci".to_owned()),
            },
        )
        .unwrap();
        let reqs = h.requests.lock().unwrap();
        let post = reqs.iter().find(|r| r.method == Method::Post).unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(post.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["key"], "ssh-ed25519 AAAA...");
        assert_eq!(body["label"], "ci");
        assert!(bufs.stdout_string().contains("✓ Added deploy key 5"));
    }

    #[test]
    fn add_without_key_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        let (ctx, _bufs) = ctx_with(h, authed());
        let err = add(
            &ctx,
            AddArgs {
                key: None,
                key_file: None,
                title: None,
            },
        )
        .unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
    }

    #[test]
    fn delete_hits_id_path() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "delete",
            FakeTransport::rest(Method::Delete, "/repositories/acme/widgets/deploy-keys/7"),
            FakeTransport::json(204, ""),
        );
        let (ctx, bufs) = ctx_with(h, authed());
        delete(&ctx, DeleteArgs { id: 7 }).unwrap();
        assert!(bufs.stdout_string().contains("✓ Deleted deploy key 7"));
    }
}
