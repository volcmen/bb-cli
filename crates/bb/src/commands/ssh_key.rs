//! `bb ssh-key` — manage your account's SSH keys.

use crate::api::models::User;
use crate::api::BitbucketClient;
use crate::core::{AuthError, Context, FlagError};
use crate::render::percent_encode;
use clap::{Args, Subcommand};

#[derive(Args, Debug)]
pub struct SshKeyArgs {
    #[command(subcommand)]
    command: SshKeyCommands,
}

#[derive(Subcommand, Debug)]
enum SshKeyCommands {
    /// List your SSH keys
    List,
    /// Add an SSH public key (from a file or `-` for stdin)
    Add(AddArgs),
    /// Delete an SSH key by its uuid
    Delete(DeleteArgs),
}

#[derive(Args, Debug)]
pub struct AddArgs {
    /// Path to the public key file (use "-" for stdin)
    #[arg(value_name = "PATH")]
    pub path: String,
    /// Label for the key
    #[arg(long)]
    pub title: Option<String>,
}

#[derive(Args, Debug)]
pub struct DeleteArgs {
    /// Key uuid
    #[arg(value_name = "KEY-UUID")]
    pub uuid: String,
}

#[derive(serde::Deserialize)]
struct SshKey {
    /// Stable identifier (`{...}`), required by `bb ssh-key delete <KEY-UUID>`.
    #[serde(default)]
    uuid: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    key: Option<String>,
}

#[derive(serde::Serialize)]
struct AddKeyBody<'a> {
    key: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<&'a str>,
}

/// Run `bb ssh-key <sub>`.
///
/// # Errors
/// [`AuthError`] (4) unauthenticated; [`FlagError`] (1) for IO/usage errors;
/// propagates [`ApiError`](crate::core::ApiError).
pub fn run(ctx: &Context, args: SshKeyArgs) -> anyhow::Result<()> {
    let host = ctx.host();
    let Some(header) = crate::auth::header_for(ctx.config.as_ref(), &host) else {
        return Err(AuthError::new(host).into());
    };
    let client = BitbucketClient::new(ctx.transport.clone(), Some(header));

    let me: User = client.get("/user")?;
    let uuid = me.uuid.unwrap_or_default();
    let base = format!("/users/{}/ssh-keys", percent_encode(&uuid));

    match args.command {
        SshKeyCommands::List => list(ctx, &client, &base),
        SshKeyCommands::Add(a) => add(ctx, &client, &base, &a),
        SshKeyCommands::Delete(a) => delete(ctx, &client, &base, &a.uuid),
    }
}

fn list(ctx: &Context, client: &BitbucketClient, base: &str) -> anyhow::Result<()> {
    let keys: Vec<SshKey> = client.paginate(base, None)?;
    if keys.is_empty() {
        ctx.io.println("No SSH keys.");
        return Ok(());
    }
    for k in keys {
        // uuid first so the list → `bb ssh-key delete <KEY-UUID>` workflow is
        // copy-pastable; layout is `uuid\tlabel\tpublickey`.
        let uuid = k.uuid.as_deref().unwrap_or_default();
        let label = k.label.as_deref().unwrap_or("(no label)");
        let key = k.key.as_deref().unwrap_or_default();
        ctx.io.println(&format!("{uuid}\t{label}\t{key}"));
    }
    Ok(())
}

fn add(ctx: &Context, client: &BitbucketClient, base: &str, args: &AddArgs) -> anyhow::Result<()> {
    let key_text = if args.path == "-" {
        ctx.io.read_stdin_to_string()?
    } else {
        std::fs::read_to_string(&args.path)?
    };
    let key = key_text.trim();
    if key.is_empty() {
        return Err(FlagError::new("the SSH public key is empty").into());
    }
    let body = AddKeyBody {
        key,
        label: args.title.as_deref(),
    };
    let _resp: serde_json::Value = client.post(base, &body)?;
    ctx.io.println("✓ Added SSH key");
    Ok(())
}

fn delete(ctx: &Context, client: &BitbucketClient, base: &str, uuid: &str) -> anyhow::Result<()> {
    let path = format!("{base}/{}", percent_encode(uuid));
    client.send_empty(crate::core::Method::Delete, &path)?;
    ctx.io.println(&format!("✓ Deleted SSH key {uuid}"));
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

    fn stub_user(h: &Arc<FakeTransport>) {
        h.stub(
            "user",
            FakeTransport::rest(Method::Get, "/2.0/user"),
            FakeTransport::json(200, r#"{"uuid":"{abc}"}"#),
        );
    }

    fn list_args() -> SshKeyArgs {
        SshKeyArgs {
            command: SshKeyCommands::List,
        }
    }

    #[test]
    fn list_prints_keys() {
        let h = Arc::new(FakeTransport::new());
        stub_user(&h);
        h.stub(
            "list keys",
            FakeTransport::rest(Method::Get, "/ssh-keys"),
            FakeTransport::json(
                200,
                r#"{"values":[{"uuid":"{k1}","label":"laptop","key":"ssh-rsa AAAA"}]}"#,
            ),
        );
        let (ctx, bufs) = ctx_with(h.clone(), authed_config());
        run(&ctx, list_args()).unwrap();
        let out = bufs.stdout_string();
        // uuid (needed by `delete`) leads, then label, then the key.
        assert!(out.contains("{k1}\tlaptop\tssh-rsa AAAA"), "out: {out}");
    }

    #[test]
    fn list_empty() {
        let h = Arc::new(FakeTransport::new());
        stub_user(&h);
        h.stub(
            "list empty",
            FakeTransport::rest(Method::Get, "/ssh-keys"),
            FakeTransport::json(200, r#"{"values":[]}"#),
        );
        let (ctx, bufs) = ctx_with(h.clone(), authed_config());
        run(&ctx, list_args()).unwrap();
        assert!(bufs.stdout_string().contains("No SSH keys."));
    }

    #[test]
    fn add_posts_key_from_file() {
        let h = Arc::new(FakeTransport::new());
        stub_user(&h);
        h.stub(
            "add key",
            FakeTransport::rest(Method::Post, "/ssh-keys"),
            FakeTransport::json(201, r#"{"uuid":"{k1}"}"#),
        );
        let (ctx, bufs) = ctx_with(h.clone(), authed_config());

        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("id.pub");
        std::fs::write(&f, "ssh-ed25519 AAAAKEY me@host\n").unwrap();

        run(
            &ctx,
            SshKeyArgs {
                command: SshKeyCommands::Add(AddArgs {
                    path: f.to_string_lossy().into_owned(),
                    title: Some("work".to_owned()),
                }),
            },
        )
        .unwrap();

        let reqs = h.requests.lock().unwrap();
        let post = reqs.iter().find(|r| r.method == Method::Post).unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(post.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["key"], "ssh-ed25519 AAAAKEY me@host");
        assert_eq!(body["label"], "work");
        assert!(bufs.stdout_string().contains("✓ Added SSH key"));
    }

    #[test]
    fn delete_sends_delete() {
        let h = Arc::new(FakeTransport::new());
        stub_user(&h);
        h.stub(
            "delete key",
            FakeTransport::rest(Method::Delete, "/ssh-keys/"),
            FakeTransport::json(204, ""),
        );
        let (ctx, bufs) = ctx_with(h.clone(), authed_config());
        run(
            &ctx,
            SshKeyArgs {
                command: SshKeyCommands::Delete(DeleteArgs {
                    uuid: "{k1}".to_owned(),
                }),
            },
        )
        .unwrap();
        assert!(bufs.stdout_string().contains("✓ Deleted SSH key {k1}"));
    }

    #[test]
    fn not_authed_is_auth_error() {
        let h = Arc::new(FakeTransport::new());
        let (ctx, _bufs) = ctx_with(h.clone(), Arc::new(FileConfig::blank()));
        let err = run(&ctx, list_args()).unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some(), "got: {err}");
    }
}
