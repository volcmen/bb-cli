//! `bb variable` — manage Bitbucket Pipelines variables (repo or workspace).

use crate::api::BitbucketClient;
use crate::core::{AuthError, Context, FlagError, Method};
use clap::{Args, Subcommand};

#[derive(Args, Debug)]
pub struct VariableArgs {
    #[command(subcommand)]
    command: VariableCommands,
}

#[derive(Subcommand, Debug)]
enum VariableCommands {
    /// List variables
    List(ScopeArgs),
    /// Set (create or update) a variable
    Set(SetArgs),
    /// Delete a variable by key
    Delete(DeleteArgs),
}

#[derive(Args, Debug)]
pub struct ScopeArgs {
    /// Target workspace variables instead of the current repo's
    #[arg(long, value_name = "WS")]
    pub workspace: Option<String>,
}

#[derive(Args, Debug)]
pub struct SetArgs {
    /// Variable key
    #[arg(value_name = "KEY")]
    pub key: String,
    /// Variable value
    #[arg(long)]
    pub value: String,
    /// Store as a secured (write-only) variable
    #[arg(long)]
    pub secured: bool,
    /// Target workspace variables instead of the current repo's
    #[arg(long, value_name = "WS")]
    pub workspace: Option<String>,
}

#[derive(Args, Debug)]
pub struct DeleteArgs {
    /// Variable key
    #[arg(value_name = "KEY")]
    pub key: String,
    /// Target workspace variables instead of the current repo's
    #[arg(long, value_name = "WS")]
    pub workspace: Option<String>,
}

#[derive(serde::Deserialize)]
struct Variable {
    #[serde(default)]
    uuid: Option<String>,
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    value: Option<String>,
    #[serde(default)]
    secured: bool,
}

#[derive(serde::Serialize)]
struct VariableBody<'a> {
    key: &'a str,
    value: &'a str,
    secured: bool,
}

/// Run `bb variable <sub>`.
///
/// # Errors
/// [`AuthError`] (4) unauthenticated; [`FlagError`] (1) deleting a missing key;
/// propagates [`ApiError`](crate::core::ApiError).
pub fn run(ctx: &Context, args: VariableArgs) -> anyhow::Result<()> {
    match args.command {
        VariableCommands::List(a) => {
            let (client, base) = setup(ctx, &a.workspace)?;
            list(ctx, &client, &base)
        }
        VariableCommands::Set(a) => {
            let (client, base) = setup(ctx, &a.workspace)?;
            set(ctx, &client, &base, &a)
        }
        VariableCommands::Delete(a) => {
            let (client, base) = setup(ctx, &a.workspace)?;
            delete(ctx, &client, &base, &a.key)
        }
    }
}

/// Build the client and the variables base path for the chosen scope.
fn setup(ctx: &Context, workspace: &Option<String>) -> anyhow::Result<(BitbucketClient, String)> {
    let host = ctx.host();
    let Some(header) = crate::auth::header_for(ctx.config.as_ref(), &host) else {
        return Err(AuthError::new(host).into());
    };
    let client = BitbucketClient::new(ctx.transport.clone(), Some(header));
    let base = match workspace {
        // Workspace endpoint uses a hyphen ("pipelines-config"); repo uses "_".
        Some(ws) => format!("/workspaces/{ws}/pipelines-config/variables"),
        None => {
            let repo = ctx.base_repo()?;
            format!(
                "/repositories/{}/{}/pipelines_config/variables",
                repo.workspace(),
                repo.slug()
            )
        }
    };
    Ok((client, base))
}

fn fetch_all(client: &BitbucketClient, base: &str) -> anyhow::Result<Vec<Variable>> {
    Ok(client.paginate(base, None)?)
}

fn list(ctx: &Context, client: &BitbucketClient, base: &str) -> anyhow::Result<()> {
    let vars = fetch_all(client, base)?;
    if vars.is_empty() {
        ctx.io.println("No variables.");
        return Ok(());
    }
    for v in vars {
        let key = v.key.as_deref().unwrap_or_default();
        let shown = if v.secured {
            "(secured)".to_owned()
        } else {
            v.value.unwrap_or_default()
        };
        ctx.io.println(&format!("{key}\t{shown}"));
    }
    Ok(())
}

fn set(ctx: &Context, client: &BitbucketClient, base: &str, args: &SetArgs) -> anyhow::Result<()> {
    let body = VariableBody {
        key: &args.key,
        value: &args.value,
        secured: args.secured,
    };
    let existing = fetch_all(client, base)?
        .into_iter()
        .find(|v| v.key.as_deref() == Some(args.key.as_str()))
        .and_then(|v| v.uuid);

    let _resp: serde_json::Value = match existing {
        Some(uuid) => client.put(
            &format!("{base}/{}", crate::render::percent_encode(&uuid)),
            &body,
        )?,
        None => client.post(base, &body)?,
    };
    ctx.io.println(&format!("✓ Set variable {}", args.key));
    Ok(())
}

fn delete(ctx: &Context, client: &BitbucketClient, base: &str, key: &str) -> anyhow::Result<()> {
    let uuid = fetch_all(client, base)?
        .into_iter()
        .find(|v| v.key.as_deref() == Some(key))
        .and_then(|v| v.uuid)
        .ok_or_else(|| FlagError::new(format!("no variable named {key:?}")))?;
    client.send_empty(
        Method::Delete,
        &format!("{base}/{}", crate::render::percent_encode(&uuid)),
    )?;
    ctx.io.println(&format!("✓ Deleted variable {key}"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::api::testing::FakeTransport;
    use crate::config::FileConfig;
    use crate::core::{ConfigProvider, GitClient, RepoId, Transport};
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

    const REPO_VARS_PATH: &str = "/repositories/acme/widgets/pipelines_config/variables";

    #[test]
    fn list_prints_variables() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list",
            FakeTransport::rest(Method::Get, REPO_VARS_PATH),
            FakeTransport::json(
                200,
                r#"{"values":[{"key":"A","value":"1"},{"key":"TOKEN","secured":true}]}"#,
            ),
        );
        let (ctx, bufs) = ctx_with(h.clone(), authed_config());
        run(
            &ctx,
            VariableArgs {
                command: VariableCommands::List(ScopeArgs { workspace: None }),
            },
        )
        .unwrap();
        let out = bufs.stdout_string();
        assert!(out.contains("A\t1"), "out: {out}");
        assert!(out.contains("TOKEN\t(secured)"), "out: {out}");
    }

    #[test]
    fn set_creates_when_absent() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list",
            FakeTransport::rest(Method::Get, REPO_VARS_PATH),
            FakeTransport::json(200, r#"{"values":[]}"#),
        );
        h.stub(
            "create",
            FakeTransport::rest(Method::Post, REPO_VARS_PATH),
            FakeTransport::json(201, r#"{"uuid":"{v1}"}"#),
        );
        let (ctx, bufs) = ctx_with(h.clone(), authed_config());
        run(
            &ctx,
            VariableArgs {
                command: VariableCommands::Set(SetArgs {
                    key: "A".to_owned(),
                    value: "1".to_owned(),
                    secured: false,
                    workspace: None,
                }),
            },
        )
        .unwrap();
        let reqs = h.requests.lock().unwrap();
        let post = reqs.iter().find(|r| r.method == Method::Post).unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(post.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["key"], "A");
        assert_eq!(body["value"], "1");
        assert_eq!(body["secured"], false);
        assert!(bufs.stdout_string().contains("✓ Set variable A"));
    }

    #[test]
    fn set_updates_when_present() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list",
            FakeTransport::rest(Method::Get, REPO_VARS_PATH),
            FakeTransport::json(
                200,
                r#"{"values":[{"uuid":"{v1}","key":"A","value":"old"}]}"#,
            ),
        );
        h.stub(
            "update",
            FakeTransport::rest(Method::Put, "/pipelines_config/variables/"),
            FakeTransport::json(200, r#"{"uuid":"{v1}"}"#),
        );
        let (ctx, _bufs) = ctx_with(h.clone(), authed_config());
        run(
            &ctx,
            VariableArgs {
                command: VariableCommands::Set(SetArgs {
                    key: "A".to_owned(),
                    value: "new".to_owned(),
                    secured: true,
                    workspace: None,
                }),
            },
        )
        .unwrap();
        let reqs = h.requests.lock().unwrap();
        let put = reqs
            .iter()
            .find(|r| r.method == Method::Put)
            .expect("a PUT");
        assert!(
            put.url.contains("%7Bv1%7D"),
            "PUT to uuid path: {}",
            put.url
        );
        let body: serde_json::Value = serde_json::from_slice(put.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["secured"], true);
    }

    #[test]
    fn delete_resolves_key_and_deletes() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list",
            FakeTransport::rest(Method::Get, REPO_VARS_PATH),
            FakeTransport::json(200, r#"{"values":[{"uuid":"{v1}","key":"A"}]}"#),
        );
        h.stub(
            "del",
            FakeTransport::rest(Method::Delete, "/pipelines_config/variables/"),
            FakeTransport::json(204, ""),
        );
        let (ctx, bufs) = ctx_with(h.clone(), authed_config());
        run(
            &ctx,
            VariableArgs {
                command: VariableCommands::Delete(DeleteArgs {
                    key: "A".to_owned(),
                    workspace: None,
                }),
            },
        )
        .unwrap();
        assert!(bufs.stdout_string().contains("✓ Deleted variable A"));
    }

    #[test]
    fn delete_missing_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list",
            FakeTransport::rest(Method::Get, REPO_VARS_PATH),
            FakeTransport::json(200, r#"{"values":[]}"#),
        );
        let (ctx, _bufs) = ctx_with(h.clone(), authed_config());
        let err = run(
            &ctx,
            VariableArgs {
                command: VariableCommands::Delete(DeleteArgs {
                    key: "MISSING".to_owned(),
                    workspace: None,
                }),
            },
        )
        .unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
    }

    #[test]
    fn workspace_scope_uses_workspace_endpoint() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "ws list",
            FakeTransport::rest(Method::Get, "/workspaces/myws/pipelines-config/variables"),
            FakeTransport::json(200, r#"{"values":[]}"#),
        );
        let (ctx, bufs) = ctx_with(h.clone(), authed_config());
        run(
            &ctx,
            VariableArgs {
                command: VariableCommands::List(ScopeArgs {
                    workspace: Some("myws".to_owned()),
                }),
            },
        )
        .unwrap();
        assert!(bufs.stdout_string().contains("No variables."));
    }

    #[test]
    fn not_authed_is_auth_error() {
        let h = Arc::new(FakeTransport::new());
        let (ctx, _bufs) = ctx_with(h.clone(), Arc::new(FileConfig::blank()));
        let err = run(
            &ctx,
            VariableArgs {
                command: VariableCommands::List(ScopeArgs { workspace: None }),
            },
        )
        .unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some(), "got: {err}");
    }
}
