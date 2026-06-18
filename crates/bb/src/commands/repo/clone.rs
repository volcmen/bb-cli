//! `bb repo clone` — clone a Bitbucket repository via `git`.

use crate::api::models::Repository;
use crate::api::BitbucketClient;
use crate::core::{AuthError, Context, FlagError, RepoId};
use clap::Args;

use crate::auth;

#[derive(Args, Debug)]
pub struct CloneArgs {
    /// Repository as WORKSPACE/SLUG
    // `id = "source"` avoids colliding with the global `-R/--repo` (clap id
    // `repo`), which otherwise shadows the global short under this subcommand.
    #[arg(id = "source", value_name = "WORKSPACE/SLUG")]
    pub repo: String,
    /// Target directory (defaults to the repo slug)
    #[arg(value_name = "DIRECTORY")]
    pub dir: Option<String>,
    /// Clone protocol (overrides the `git_protocol` config; default https)
    #[arg(long, value_parser = ["ssh", "https"])]
    pub protocol: Option<String>,
}

/// Run `bb repo clone`.
///
/// # Errors
/// Returns [`AuthError`] (exit 4) if not authenticated for the repo's host,
/// [`FlagError`] (exit 1) for a malformed target, when the repository is not
/// found, or when no clone URL is available; propagates
/// [`ApiError`](crate::core::ApiError) and [`GitError`](crate::core::GitError).
pub fn run(ctx: &Context, args: CloneArgs) -> anyhow::Result<()> {
    let repo: RepoId = args
        .repo
        .parse()
        .map_err(|e| anyhow::Error::from(FlagError::new(e)))?;
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

    // Protocol precedence: --protocol flag, then `git_protocol` config, then
    // https. Falls back to the other protocol if the chosen one has no URL.
    let protocol = args
        .protocol
        .clone()
        .or_else(|| ctx.config.get("", "git_protocol"))
        .unwrap_or_else(|| "https".to_owned());
    let fallback = if protocol == "ssh" { "https" } else { "ssh" };
    let url = repository
        .clone_url(&protocol)
        .or_else(|| repository.clone_url(fallback))
        .ok_or_else(|| {
            FlagError::new(format!(
                "no clone URL available for {}/{}",
                repo.workspace(),
                repo.slug()
            ))
        })?;

    ctx.git.clone_repo(url, args.dir.as_deref())?;
    ctx.io
        .println(&format!("✓ Cloned {}/{}", repo.workspace(), repo.slug()));
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

    /// A git stub whose only expectation is a `git clone` matching `pattern`.
    fn git_expecting(pattern: &str) -> Arc<dyn GitClient> {
        let s = Arc::new(StubRunner::new());
        s.register(pattern, 0, "");
        Arc::new(ShellGit::new(s))
    }

    /// A git stub that must never be called (e.g. auth/not-found paths).
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

    /// As [`config`], but with `git_protocol = ssh` (a global key).
    fn config_ssh() -> Arc<dyn ConfigProvider> {
        let cfg = FileConfig::blank();
        cfg.set("bitbucket.org", "auth_type", "app_password")
            .unwrap();
        cfg.set("bitbucket.org", "username", "u").unwrap();
        cfg.set("bitbucket.org", "token", "t").unwrap();
        cfg.set("", "git_protocol", "ssh").unwrap();
        Arc::new(cfg)
    }

    fn args(repo: &str, dir: Option<&str>) -> CloneArgs {
        CloneArgs {
            repo: repo.to_owned(),
            dir: dir.map(ToOwned::to_owned),
            protocol: None,
        }
    }

    const WIDGETS: &str = r#"{
        "slug": "widgets",
        "full_name": "acme/widgets",
        "is_private": true,
        "links": {
            "clone": [
                {"name": "https", "href": "https://bitbucket.org/acme/widgets.git"},
                {"name": "ssh", "href": "git@bitbucket.org:acme/widgets.git"}
            ]
        }
    }"#;

    fn stub_repo(h: &Arc<FakeTransport>) {
        h.stub(
            "get repo",
            FakeTransport::rest(Method::Get, "/repositories/acme/widgets"),
            FakeTransport::json(200, WIDGETS),
        );
    }

    #[test]
    fn clone_https_by_default() {
        let h = Arc::new(FakeTransport::new());
        stub_repo(&h);
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        // StubRunner Drop asserts this `git clone <https-url>` was hit.
        let git = git_expecting(r"^git clone -- https://bitbucket\.org/acme/widgets\.git$");
        let (ctx, bufs) = test_context(transport, git, config(), prompter, false);

        run(&ctx, args("acme/widgets", None)).unwrap();
        assert!(bufs.stdout_string().contains("✓ Cloned acme/widgets"));
    }

    #[test]
    fn clone_ssh_per_config() {
        let h = Arc::new(FakeTransport::new());
        stub_repo(&h);
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let git = git_expecting(r"^git clone -- git@bitbucket\.org:acme/widgets\.git$");
        let (ctx, bufs) = test_context(transport, git, config_ssh(), prompter, false);

        run(&ctx, args("acme/widgets", None)).unwrap();
        assert!(bufs.stdout_string().contains("✓ Cloned acme/widgets"));
    }

    #[test]
    fn clone_protocol_flag_overrides_config_to_ssh() {
        // #92: default (https) config, but --protocol ssh wins.
        let h = Arc::new(FakeTransport::new());
        stub_repo(&h);
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let git = git_expecting(r"^git clone -- git@bitbucket\.org:acme/widgets\.git$");
        let (ctx, _bufs) = test_context(transport, git, config(), prompter, false);

        let a = CloneArgs {
            protocol: Some("ssh".to_owned()),
            ..args("acme/widgets", None)
        };
        run(&ctx, a).unwrap();
    }

    #[test]
    fn clone_protocol_flag_overrides_config_to_https() {
        // git_protocol=ssh config, but --protocol https wins.
        let h = Arc::new(FakeTransport::new());
        stub_repo(&h);
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let git = git_expecting(r"^git clone -- https://bitbucket\.org/acme/widgets\.git$");
        let (ctx, _bufs) = test_context(transport, git, config_ssh(), prompter, false);

        let a = CloneArgs {
            protocol: Some("https".to_owned()),
            ..args("acme/widgets", None)
        };
        run(&ctx, a).unwrap();
    }

    #[test]
    fn clone_passes_target_directory() {
        let h = Arc::new(FakeTransport::new());
        stub_repo(&h);
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let git = git_expecting(r"^git clone -- https://bitbucket\.org/acme/widgets\.git mydir$");
        let (ctx, _bufs) = test_context(transport, git, config(), prompter, false);

        run(&ctx, args("acme/widgets", Some("mydir"))).unwrap();
    }

    #[test]
    fn clone_falls_back_when_preferred_protocol_missing() {
        // git_protocol=ssh but only an https URL exists → fall back to https.
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get repo",
            FakeTransport::rest(Method::Get, "/repositories/acme/widgets"),
            FakeTransport::json(
                200,
                r#"{"slug":"widgets","full_name":"acme/widgets",
                    "links":{"clone":[
                        {"name":"https","href":"https://bitbucket.org/acme/widgets.git"}]}}"#,
            ),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let git = git_expecting(r"^git clone -- https://bitbucket\.org/acme/widgets\.git$");
        let (ctx, _bufs) = test_context(transport, git, config_ssh(), prompter, false);

        run(&ctx, args("acme/widgets", None)).unwrap();
    }

    #[test]
    fn clone_no_url_available_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get repo",
            FakeTransport::rest(Method::Get, "/repositories/acme/widgets"),
            FakeTransport::json(
                200,
                r#"{"slug":"widgets","full_name":"acme/widgets","links":{"clone":[]}}"#,
            ),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, _bufs) = test_context(transport, no_git(), config(), prompter, false);

        let err = run(&ctx, args("acme/widgets", None)).unwrap_err();
        let flag = err.downcast_ref::<FlagError>();
        assert!(flag.is_some(), "expected FlagError, got: {err}");
        assert!(flag.unwrap().0.contains("no clone URL available"));
    }

    #[test]
    fn clone_not_found_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        h.stub(
            "get repo 404",
            FakeTransport::rest(Method::Get, "/repositories/acme/nope"),
            FakeTransport::json(404, r#"{"error":{"message":"not found"}}"#),
        );
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, _bufs) = test_context(transport, no_git(), config(), prompter, false);

        let err = run(&ctx, args("acme/nope", None)).unwrap_err();
        let flag = err.downcast_ref::<FlagError>();
        assert!(flag.is_some(), "expected FlagError, got: {err}");
        assert!(flag.unwrap().0.contains("not found"));
    }

    #[test]
    fn clone_invalid_target_is_flag_error() {
        let h = Arc::new(FakeTransport::new());
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, _bufs) = test_context(transport, no_git(), config(), prompter, false);

        let err = run(&ctx, args("not-a-repo", None)).unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some());
    }

    #[test]
    fn clone_not_logged_in_is_auth_error() {
        let h = Arc::new(FakeTransport::new());
        let transport: Arc<dyn Transport> = h.clone();
        let prompter = Arc::new(ScriptedPrompter::new());
        let cfg: Arc<dyn ConfigProvider> = Arc::new(FileConfig::blank());
        let (ctx, _bufs) = test_context(transport, no_git(), cfg, prompter, false);

        let err = run(&ctx, args("acme/widgets", None)).unwrap_err();
        assert!(err.downcast_ref::<AuthError>().is_some());
    }
}
