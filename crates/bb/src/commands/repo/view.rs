//! `bb repo view` — show a repository's details, or open it in the browser.

use bb_api::models::Repository;
use bb_api::BitbucketClient;
use bb_core::{AuthError, Context, FlagError, RepoId};
use clap::Args;

use crate::auth;
use crate::render::sanitize;

/// JSON fields a repository can be projected to with `--json`.
const FIELDS: &[&str] = &[
    "slug",
    "name",
    "full_name",
    "is_private",
    "description",
    "mainbranch",
    "links",
];

#[derive(Args, Debug)]
pub struct ViewArgs {
    /// Repository as WORKSPACE/SLUG (defaults to the current repo)
    // `id = "target"` avoids colliding with the global `-R/--repo` (clap id
    // `repo`); a same-id local arg shadows the global's short, which made
    // `bb repo view -R …` error with "unexpected argument '-R'".
    #[arg(id = "target", value_name = "WORKSPACE/SLUG")]
    pub repo: Option<String>,
    /// Open the repository in the browser
    #[arg(long)]
    pub web: bool,
    #[command(flatten)]
    pub json: crate::output::JsonFlags,
}

/// Run `bb repo view`.
///
/// # Errors
/// Returns [`AuthError`] (exit 4) if not authenticated for the repo's host,
/// [`FlagError`] (exit 1) for a malformed target or when the repository is not
/// found, and propagates [`ApiError`](bb_core::ApiError) from the lookup.
pub fn run(ctx: &Context, args: ViewArgs) -> anyhow::Result<()> {
    let repo = resolve_target(ctx, args.repo.as_deref())?;
    let host = repo.host().to_owned();

    let header = auth::header_for(ctx.config.as_ref(), &host);
    if header.is_none() {
        return Err(AuthError::new(host).into());
    }
    let client = BitbucketClient::new(ctx.transport.clone(), header);

    let path = format!("/repositories/{}/{}", repo.workspace(), repo.slug());
    let repository: Repository = match client.get(&path) {
        Ok(r) => r,
        Err(e) if e.is_not_found() => {
            return Err(FlagError::new(format!(
                "repository {}/{} not found",
                repo.workspace(),
                repo.slug()
            ))
            .into());
        }
        Err(e) => return Err(e.into()),
    };

    if args.json.requested() {
        args.json.validate(FIELDS)?;
        args.json
            .emit(&ctx.io, serde_json::to_value(&repository)?)?;
        return Ok(());
    }

    if args.web {
        let url = repository
            .html_url()
            .ok_or_else(|| FlagError::new("no browser URL is available for this repository"))?;
        ctx.browser.browse(url)?;
        ctx.io.println(&format!("Opening {url} in your browser."));
        return Ok(());
    }

    ctx.io.print(&render_view(&repo, &repository));
    Ok(())
}

/// Resolve the repository the command targets: parse `WORKSPACE/SLUG` if given,
/// else fall back to the current repo (`ctx.base_repo()`).
fn resolve_target(ctx: &Context, arg: Option<&str>) -> anyhow::Result<RepoId> {
    match arg {
        Some(s) => s.parse::<RepoId>().map_err(|e| FlagError::new(e).into()),
        None => Ok(ctx.base_repo()?),
    }
}

/// Render a repository's details.
fn render_view(repo: &RepoId, r: &Repository) -> String {
    let full_name = r.full_name.clone().unwrap_or_else(|| repo.full_name());
    let visibility = match r.is_private {
        Some(true) => "private",
        _ => "public",
    };
    let description = r
        .description
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map_or_else(|| "No description.".to_owned(), sanitize);
    let branch = r.mainbranch.as_ref().map_or("", |b| b.name.as_str());
    let url = r.html_url().unwrap_or("");

    let mut out = format!("{}\n", sanitize(&full_name));
    out.push_str(&format!("{visibility}\n"));
    out.push('\n');
    out.push_str(&format!("{description}\n"));
    out.push('\n');
    if !branch.is_empty() {
        out.push_str(&format!("Default branch: {}\n", sanitize(branch)));
    }
    if !url.is_empty() {
        out.push_str(&format!("{url}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bb_api::testing::FakeTransport;
    use bb_config::FileConfig;
    use bb_core::{ConfigProvider, GitClient, Method, RepoId, Transport};
    use bb_git::{ShellGit, StubRunner};

    use super::*;
    use crate::testsupport::{test_context, ScriptedPrompter};

    /// Git stub that answers `remote -v` (so `base_repo` can default).
    fn git() -> Arc<dyn GitClient> {
        let s = Arc::new(StubRunner::new());
        s.register(
            "remote -v",
            0,
            "origin\tgit@bitbucket.org:acme/widgets.git (fetch)\n\
             origin\tgit@bitbucket.org:acme/widgets.git (push)\n",
        );
        Arc::new(ShellGit::new(s))
    }

    /// Git stub that must never be called (the target comes from the arg).
    fn no_git() -> Arc<dyn GitClient> {
        Arc::new(ShellGit::new(Arc::new(StubRunner::new())))
    }

    fn config() -> Arc<dyn ConfigProvider> {
        let cfg = FileConfig::blank();
        cfg.set("bitbucket.org", "auth_type", "app_password")
            .unwrap();
        cfg.set("bitbucket.org", "username", "u").unwrap();
        cfg.set("bitbucket.org", "token", "t").unwrap();
        Arc::new(cfg)
    }

    fn args(repo: Option<&str>, web: bool) -> ViewArgs {
        ViewArgs {
            repo: repo.map(ToOwned::to_owned),
            web,
            json: crate::output::JsonFlags::default(),
        }
    }

    const WIDGETS: &str = r#"{
        "slug": "widgets",
        "name": "widgets",
        "full_name": "acme/widgets",
        "is_private": true,
        "description": "A widget factory.",
        "mainbranch": {"name": "main"},
        "links": {
            "html": {"href": "https://bitbucket.org/acme/widgets"},
            "clone": [
                {"name": "https", "href": "https://bitbucket.org/acme/widgets.git"},
                {"name": "ssh", "href": "git@bitbucket.org:acme/widgets.git"}
            ]
        }
    }"#;

    #[test]
    fn view_by_workspace_slug_renders() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get repo",
            FakeTransport::rest(Method::Get, "/repositories/acme/widgets"),
            FakeTransport::json(200, WIDGETS),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, bufs) = test_context(transport, no_git(), config(), prompter, false);

        run(&ctx, args(Some("acme/widgets"), false)).unwrap();

        let out = bufs.stdout_string();
        assert!(out.contains("acme/widgets"), "out: {out}");
        assert!(out.contains("private"), "out: {out}");
        assert!(out.contains("A widget factory."), "out: {out}");
        assert!(out.contains("Default branch: main"), "out: {out}");
        assert!(
            out.contains("https://bitbucket.org/acme/widgets"),
            "out: {out}"
        );
    }

    #[test]
    fn view_public_with_no_description_shows_placeholder() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get repo",
            FakeTransport::rest(Method::Get, "/repositories/acme/widgets"),
            FakeTransport::json(200, r#"{"full_name":"acme/widgets","is_private":false}"#),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, bufs) = test_context(transport, no_git(), config(), prompter, false);

        run(&ctx, args(Some("acme/widgets"), false)).unwrap();

        let out = bufs.stdout_string();
        assert!(out.contains("public"), "out: {out}");
        assert!(out.contains("No description."), "out: {out}");
    }

    #[test]
    fn view_defaults_to_base_repo() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get repo",
            FakeTransport::rest(Method::Get, "/repositories/acme/widgets"),
            FakeTransport::json(200, WIDGETS),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, bufs) = test_context(transport, git(), config(), prompter, false);

        run(&ctx, args(None, false)).unwrap();
        assert!(bufs.stdout_string().contains("acme/widgets"));
    }

    #[test]
    fn view_web_browses_and_prints_url() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get repo",
            FakeTransport::rest(Method::Get, "/repositories/acme/widgets"),
            FakeTransport::json(200, WIDGETS),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, bufs) = test_context(transport, no_git(), config(), prompter, false);

        run(&ctx, args(Some("acme/widgets"), true)).unwrap();

        let out = bufs.stdout_string();
        assert!(
            out.contains("https://bitbucket.org/acme/widgets"),
            "out: {out}"
        );
        // --web must not render the description.
        assert!(!out.contains("A widget factory."), "out: {out}");
    }

    #[test]
    fn view_not_found_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get repo 404",
            FakeTransport::rest(Method::Get, "/repositories/acme/nope"),
            FakeTransport::json(
                404,
                r#"{"error":{"message":"Repository acme/nope not found"}}"#,
            ),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, _bufs) = test_context(transport, no_git(), config(), prompter, false);

        let err = run(&ctx, args(Some("acme/nope"), false)).unwrap_err();
        let flag = err.downcast_ref::<FlagError>();
        assert!(flag.is_some(), "expected FlagError, got: {err}");
        assert!(flag.unwrap().0.contains("not found"));
    }

    #[test]
    fn view_invalid_target_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, _bufs) = test_context(transport, no_git(), config(), prompter, false);

        let err = run(&ctx, args(Some("not-a-repo"), false)).unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some());
    }

    #[test]
    fn view_json_emits_projected_fields() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get repo json",
            FakeTransport::rest(Method::Get, "/repositories/acme/widgets"),
            FakeTransport::json(200, WIDGETS),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, bufs) = test_context(transport, no_git(), config(), prompter, false);

        let a = ViewArgs {
            repo: Some("acme/widgets".to_owned()),
            web: false,
            json: crate::output::JsonFlags {
                json: vec!["slug".into(), "full_name".into(), "is_private".into()],
                jq: None,
                template: None,
            },
        };
        run(&ctx, a).unwrap();

        let out = bufs.stdout_string();
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(v["slug"], "widgets");
        assert_eq!(v["full_name"], "acme/widgets");
        assert_eq!(v["is_private"], true);
        // Unrequested fields are projected away.
        assert!(v.get("description").is_none(), "out: {out}");
    }

    #[test]
    fn view_json_takes_precedence_over_web() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get repo json web",
            FakeTransport::rest(Method::Get, "/repositories/acme/widgets"),
            FakeTransport::json(200, WIDGETS),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, bufs) = test_context(transport, no_git(), config(), prompter, false);

        let a = ViewArgs {
            repo: Some("acme/widgets".to_owned()),
            web: true,
            json: crate::output::JsonFlags {
                json: vec!["slug".into()],
                jq: None,
                template: None,
            },
        };
        run(&ctx, a).unwrap();

        let out = bufs.stdout_string();
        // --json wins: JSON is emitted, no browser-open message.
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(v["slug"], "widgets");
        assert!(!out.contains("Opening"), "out: {out}");
    }

    #[test]
    fn view_json_unknown_field_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get repo json bogus",
            FakeTransport::rest(Method::Get, "/repositories/acme/widgets"),
            FakeTransport::json(200, WIDGETS),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, _bufs) = test_context(transport, no_git(), config(), prompter, false);

        let a = ViewArgs {
            repo: Some("acme/widgets".to_owned()),
            web: false,
            json: crate::output::JsonFlags {
                json: vec!["bogus".into()],
                jq: None,
                template: None,
            },
        };
        let err = run(&ctx, a).unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some());
    }

    #[test]
    fn view_not_logged_in_is_auth_error() {
        let h = Arc::new(FakeTransport::new());
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let cfg: Arc<dyn ConfigProvider> = Arc::new(FileConfig::blank());
        let (ctx, _bufs) = test_context(transport, no_git(), cfg, prompter, false);

        let err = run(&ctx, args(Some("acme/widgets"), false)).unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some());
    }

    #[test]
    fn render_view_sanitizes_description() {
        let r: Repository = serde_json::from_str(
            r#"{"full_name":"acme/widgets","is_private":false,
                "description":"line1\nline2"}"#,
        )
        .unwrap();
        let out = render_view(&RepoId::new("acme", "widgets"), &r);
        assert!(out.contains("line1 line2"), "out: {out}");
        assert!(!out.contains("line1\nline2"));
    }
}
