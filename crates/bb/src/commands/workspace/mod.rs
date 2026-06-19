//! `bb workspace` — inspect Bitbucket Workspaces (closest `gh org` analog).
//!
//! Bitbucket has permanently removed workspace *enumeration* (CHANGE-2770): both
//! `GET /2.0/workspaces` and the former replacement
//! `GET /2.0/user/permissions/workspaces` now return `410 Gone`, so `list` can't
//! enumerate and surfaces a clear hint instead. Members and projects still work
//! against the (non-deprecated) `/2.0/workspaces/{ws}/…` endpoints.

use crate::api::models::{Membership, Project, WorkspaceMembership};
use crate::api::BitbucketClient;
use crate::core::{AuthError, Context, FlagError};
use clap::{Args, Subcommand};

/// JSON fields a workspace membership can be projected to with `--json`
/// (the serialized keys of [`WorkspaceMembership`]).
const LIST_FIELDS: &[&str] = &["permission", "workspace"];

/// JSON fields a member can be projected to with `--json`
/// (the serialized keys of [`Membership`]).
const MEMBER_FIELDS: &[&str] = &["user"];

/// JSON fields a project can be projected to with `--json`
/// (the serialized keys of [`Project`]).
const PROJECT_FIELDS: &[&str] = &["key", "name", "is_private", "description", "links"];

#[derive(Args, Debug)]
pub struct WorkspaceArgs {
    #[command(subcommand)]
    command: WorkspaceCommands,
}

#[derive(Subcommand, Debug)]
enum WorkspaceCommands {
    /// List the workspaces you belong to
    List(ListArgs),
    /// List a workspace's members
    Members(ScopedArgs),
    /// List a workspace's projects
    Projects(ScopedArgs),
}

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Maximum number to list
    #[arg(long, default_value_t = 30)]
    pub limit: usize,
    #[command(flatten)]
    pub json: crate::output::JsonFlags,
}

#[derive(Args, Debug)]
pub struct ScopedArgs {
    /// The workspace slug
    #[arg(value_name = "WORKSPACE")]
    pub workspace: String,
    /// Maximum number to list
    #[arg(long, default_value_t = 30)]
    pub limit: usize,
    #[command(flatten)]
    pub json: crate::output::JsonFlags,
}

/// Dispatch `bb workspace <sub>`.
///
/// # Errors
/// Propagates the sub-command's error.
pub fn run(ctx: &Context, args: WorkspaceArgs) -> anyhow::Result<()> {
    match args.command {
        WorkspaceCommands::List(a) => list(ctx, a),
        WorkspaceCommands::Members(a) => members(ctx, a),
        WorkspaceCommands::Projects(a) => projects(ctx, a),
    }
}

fn client_for(ctx: &Context) -> anyhow::Result<BitbucketClient> {
    let host = ctx.config.default_host();
    let Some(header) = crate::auth::header_for(ctx.config.as_ref(), &host) else {
        return Err(AuthError::new(host).into());
    };
    Ok(BitbucketClient::new(ctx.transport.clone(), Some(header)))
}

fn list(ctx: &Context, args: ListArgs) -> anyhow::Result<()> {
    let client = client_for(ctx)?;
    let memberships: Vec<WorkspaceMembership> =
        match client.paginate("/user/permissions/workspaces", Some(args.limit)) {
            Ok(memberships) => memberships,
            // Bitbucket permanently removed workspace enumeration (CHANGE-2770):
            // the endpoint now returns 410 Gone. Surface a clear hint instead of
            // leaking the raw HTTP 410.
            Err(e) if e.is_gone() => {
                return Err(FlagError::new(
                    "Bitbucket has removed the workspace-list API (CHANGE-2770). \
                     Use `bb workspace members <ws>` or `bb workspace projects <ws>` \
                     with a known workspace slug.",
                )
                .into());
            }
            Err(e) => return Err(e.into()),
        };

    if args.json.requested() {
        args.json.validate(LIST_FIELDS)?;
        args.json
            .emit(&ctx.io, serde_json::to_value(&memberships)?)?;
        return Ok(());
    }

    if memberships.is_empty() {
        ctx.io.println("No workspaces found");
        return Ok(());
    }
    for m in &memberships {
        let ws = m.workspace.as_ref();
        let slug = ws.and_then(|w| w.slug.as_deref()).unwrap_or("?");
        let name = ws.and_then(|w| w.name.as_deref()).unwrap_or("");
        let permission = m.permission.as_deref().unwrap_or("member");
        ctx.io.println(&format!("{slug}\t{permission}\t{name}"));
    }
    Ok(())
}

fn members(ctx: &Context, args: ScopedArgs) -> anyhow::Result<()> {
    let client = client_for(ctx)?;
    let path = format!("/workspaces/{}/members", args.workspace);
    let members: Vec<Membership> = client.paginate(&path, Some(args.limit))?;

    if args.json.requested() {
        args.json.validate(MEMBER_FIELDS)?;
        args.json.emit(&ctx.io, serde_json::to_value(&members)?)?;
        return Ok(());
    }

    if members.is_empty() {
        ctx.io.println("No members found");
        return Ok(());
    }
    for m in &members {
        if let Some(user) = &m.user {
            let handle = user.username.as_deref().unwrap_or("");
            ctx.io.println(&format!("{}\t{handle}", user.label()));
        }
    }
    Ok(())
}

fn projects(ctx: &Context, args: ScopedArgs) -> anyhow::Result<()> {
    let client = client_for(ctx)?;
    let path = format!("/workspaces/{}/projects", args.workspace);
    let projects: Vec<Project> = client.paginate(&path, Some(args.limit))?;

    if args.json.requested() {
        args.json.validate(PROJECT_FIELDS)?;
        args.json.emit(&ctx.io, serde_json::to_value(&projects)?)?;
        return Ok(());
    }

    if projects.is_empty() {
        ctx.io.println("No projects found");
        return Ok(());
    }
    for p in &projects {
        let key = p.key.as_deref().unwrap_or("?");
        let privacy = if p.is_private == Some(true) {
            "private"
        } else {
            "public"
        };
        let name = p.name.as_deref().unwrap_or("");
        ctx.io.println(&format!("{key}\t{privacy}\t{name}"));
    }
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
        test_context(
            transport,
            git,
            cfg,
            Arc::new(ScriptedPrompter::new()),
            false,
        )
    }

    fn list_args() -> ListArgs {
        ListArgs {
            limit: 30,
            json: crate::output::JsonFlags::default(),
        }
    }

    fn scoped_args(workspace: &str) -> ScopedArgs {
        ScopedArgs {
            workspace: workspace.to_owned(),
            limit: 30,
            json: crate::output::JsonFlags::default(),
        }
    }

    #[test]
    fn list_renders_workspaces() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list workspaces",
            FakeTransport::rest(Method::Get, "/user/permissions/workspaces"),
            FakeTransport::json(
                200,
                r#"{"values":[
                    {"permission":"owner","workspace":{"slug":"acme","name":"Acme Inc"}},
                    {"permission":"member","workspace":{"slug":"team2","name":"Team Two"}}
                ]}"#,
            ),
        );
        let (ctx, bufs) = ctx_with(h, authed());
        list(&ctx, list_args()).unwrap();
        let out = bufs.stdout_string();
        assert!(out.contains("acme\towner\tAcme Inc"), "got: {out}");
        assert!(out.contains("team2\tmember\tTeam Two"), "got: {out}");
    }

    #[test]
    fn members_render_users() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "members",
            FakeTransport::rest(Method::Get, "/workspaces/acme/members"),
            FakeTransport::json(
                200,
                r#"{"values":[
                    {"user":{"display_name":"Alice","username":"alice"}},
                    {"user":{"display_name":"Bob","username":"bob"}}
                ]}"#,
            ),
        );
        let (ctx, bufs) = ctx_with(h, authed());
        members(&ctx, scoped_args("acme")).unwrap();
        let out = bufs.stdout_string();
        assert!(out.contains("Alice\talice"), "got: {out}");
        assert!(out.contains("Bob\tbob"), "got: {out}");
    }

    #[test]
    fn projects_render_rows() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "projects",
            FakeTransport::rest(Method::Get, "/workspaces/acme/projects"),
            FakeTransport::json(
                200,
                r#"{"values":[
                    {"key":"PROJ","name":"Project X","is_private":true},
                    {"key":"OPEN","name":"Open Source","is_private":false}
                ]}"#,
            ),
        );
        let (ctx, bufs) = ctx_with(h, authed());
        projects(&ctx, scoped_args("acme")).unwrap();
        let out = bufs.stdout_string();
        assert!(out.contains("PROJ\tprivate\tProject X"), "got: {out}");
        assert!(out.contains("OPEN\tpublic\tOpen Source"), "got: {out}");
    }

    #[test]
    fn list_not_authed_is_auth_error() {
        let h = Arc::new(FakeTransport::new());
        let (ctx, _bufs) = ctx_with(h, Arc::new(FileConfig::blank()));
        let err = list(&ctx, list_args()).unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some(), "got: {err}");
    }

    #[test]
    fn list_gone_is_friendly_flag_error() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list gone",
            FakeTransport::rest(Method::Get, "/user/permissions/workspaces"),
            FakeTransport::json(
                410,
                r#"{"type":"error","error":{"message":"CHANGE-2770: This endpoint has been removed."}}"#,
            ),
        );
        let (ctx, _bufs) = ctx_with(h, authed());
        let err = list(&ctx, list_args()).unwrap_err();

        let flag = err.downcast_ref::<FlagError>();
        assert!(flag.is_some(), "expected FlagError, got: {err}");
        let msg = &flag.unwrap().0;
        assert!(msg.contains("CHANGE-2770"), "got: {msg}");
        assert!(msg.contains("members"), "got: {msg}");
        assert!(msg.contains("projects"), "got: {msg}");
        // The raw HTTP 410 must not leak through.
        assert!(!msg.contains("HTTP 410 on https://"), "got: {msg}");
    }

    #[test]
    fn list_json_emits_projected_fields() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list json",
            FakeTransport::rest(Method::Get, "/user/permissions/workspaces"),
            FakeTransport::json(
                200,
                r#"{"values":[
                    {"permission":"owner","workspace":{"slug":"acme","name":"Acme Inc"}}
                ]}"#,
            ),
        );
        let (ctx, bufs) = ctx_with(h, authed());
        let a = ListArgs {
            json: crate::output::JsonFlags {
                json: vec!["permission".into(), "workspace".into()],
                jq: None,
                template: None,
            },
            ..list_args()
        };
        list(&ctx, a).unwrap();

        let out = bufs.stdout_string();
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        let arr = v.as_array().expect("array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["permission"], "owner");
        assert_eq!(arr[0]["workspace"]["slug"], "acme");
    }

    #[test]
    fn list_json_unknown_field_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "list json bogus",
            FakeTransport::rest(Method::Get, "/user/permissions/workspaces"),
            FakeTransport::json(200, r#"{"values":[]}"#),
        );
        let (ctx, _bufs) = ctx_with(h, authed());
        let a = ListArgs {
            json: crate::output::JsonFlags {
                json: vec!["bogus".into()],
                jq: None,
                template: None,
            },
            ..list_args()
        };
        let err = list(&ctx, a).unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
    }

    #[test]
    fn members_json_emits_projected_fields() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "members json",
            FakeTransport::rest(Method::Get, "/workspaces/acme/members"),
            FakeTransport::json(
                200,
                r#"{"values":[
                    {"user":{"display_name":"Alice","username":"alice"}}
                ]}"#,
            ),
        );
        let (ctx, bufs) = ctx_with(h, authed());
        let a = ScopedArgs {
            json: crate::output::JsonFlags {
                json: vec!["user".into()],
                jq: None,
                template: None,
            },
            ..scoped_args("acme")
        };
        members(&ctx, a).unwrap();

        let out = bufs.stdout_string();
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        let arr = v.as_array().expect("array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["user"]["username"], "alice");
    }

    #[test]
    fn projects_json_emits_projected_fields() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "projects json",
            FakeTransport::rest(Method::Get, "/workspaces/acme/projects"),
            FakeTransport::json(
                200,
                r#"{"values":[
                    {"key":"PROJ","name":"Project X","is_private":true,"description":"d"}
                ]}"#,
            ),
        );
        let (ctx, bufs) = ctx_with(h, authed());
        let a = ScopedArgs {
            json: crate::output::JsonFlags {
                json: vec!["key".into(), "name".into(), "is_private".into()],
                jq: None,
                template: None,
            },
            ..scoped_args("acme")
        };
        projects(&ctx, a).unwrap();

        let out = bufs.stdout_string();
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        let arr = v.as_array().expect("array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["key"], "PROJ");
        assert_eq!(arr[0]["name"], "Project X");
        assert_eq!(arr[0]["is_private"], true);
        // Unrequested field projected away.
        assert!(arr[0].get("description").is_none(), "got: {out}");
    }

    #[test]
    fn projects_json_unknown_field_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "projects json bogus",
            FakeTransport::rest(Method::Get, "/workspaces/acme/projects"),
            FakeTransport::json(200, r#"{"values":[]}"#),
        );
        let (ctx, _bufs) = ctx_with(h, authed());
        let a = ScopedArgs {
            json: crate::output::JsonFlags {
                json: vec!["bogus".into()],
                jq: None,
                template: None,
            },
            ..scoped_args("acme")
        };
        let err = projects(&ctx, a).unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
    }

    #[test]
    fn projects_jq_filters_full_object() {
        // `--jq`/`--template` come for free once JsonFlags is flattened — a
        // smoke test that they reach the data without explicit `--json` fields.
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "projects jq",
            FakeTransport::rest(Method::Get, "/workspaces/acme/projects"),
            FakeTransport::json(
                200,
                r#"{"values":[
                    {"key":"PROJ","name":"Project X"},
                    {"key":"OPEN","name":"Open Source"}
                ]}"#,
            ),
        );
        let (ctx, bufs) = ctx_with(h, authed());
        let a = ScopedArgs {
            json: crate::output::JsonFlags {
                json: vec![],
                jq: Some(".[].key".to_owned()),
                template: None,
            },
            ..scoped_args("acme")
        };
        projects(&ctx, a).unwrap();
        assert_eq!(bufs.stdout_string(), "\"PROJ\"\n\"OPEN\"\n");
    }

    #[test]
    fn projects_template_renders_rows() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "projects template",
            FakeTransport::rest(Method::Get, "/workspaces/acme/projects"),
            FakeTransport::json(200, r#"{"values":[{"key":"PROJ","name":"Project X"}]}"#),
        );
        let (ctx, bufs) = ctx_with(h, authed());
        let a = ScopedArgs {
            json: crate::output::JsonFlags {
                json: vec![],
                jq: None,
                template: Some("{{ for p in items }}{p.key}={p.name}{{ endfor }}".to_owned()),
            },
            ..scoped_args("acme")
        };
        projects(&ctx, a).unwrap();
        assert!(
            bufs.stdout_string().contains("PROJ=Project X"),
            "got: {}",
            bufs.stdout_string()
        );
    }
}
