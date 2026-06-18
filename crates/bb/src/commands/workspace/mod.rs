//! `bb workspace` — inspect Bitbucket Workspaces (closest `gh org` analog).
//!
//! `GET /2.0/workspaces` (list-all) is deprecated (CHANGE-2770), so `list` uses
//! the documented replacement `GET /2.0/user/permissions/workspaces`. Members and
//! projects use the (non-deprecated) `/2.0/workspaces/{ws}/…` endpoints.

use crate::api::models::{Membership, Project, WorkspaceMembership};
use crate::api::BitbucketClient;
use crate::core::{AuthError, Context};
use clap::{Args, Subcommand};

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
}

#[derive(Args, Debug)]
pub struct ScopedArgs {
    /// The workspace slug
    #[arg(value_name = "WORKSPACE")]
    pub workspace: String,
    /// Maximum number to list
    #[arg(long, default_value_t = 30)]
    pub limit: usize,
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
        client.paginate("/user/permissions/workspaces", Some(args.limit))?;

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
        list(&ctx, ListArgs { limit: 30 }).unwrap();
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
        members(
            &ctx,
            ScopedArgs {
                workspace: "acme".to_owned(),
                limit: 30,
            },
        )
        .unwrap();
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
        projects(
            &ctx,
            ScopedArgs {
                workspace: "acme".to_owned(),
                limit: 30,
            },
        )
        .unwrap();
        let out = bufs.stdout_string();
        assert!(out.contains("PROJ\tprivate\tProject X"), "got: {out}");
        assert!(out.contains("OPEN\tpublic\tOpen Source"), "got: {out}");
    }

    #[test]
    fn list_not_authed_is_auth_error() {
        let h = Arc::new(FakeTransport::new());
        let (ctx, _bufs) = ctx_with(h, Arc::new(FileConfig::blank()));
        let err = list(&ctx, ListArgs { limit: 30 }).unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some(), "got: {err}");
    }
}
