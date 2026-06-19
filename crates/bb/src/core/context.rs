//! [`Context`] — the dependency container (the analog of `gh`'s Factory).

use std::path::Path;
use std::sync::Arc;

use crate::core::error::GitError;
use crate::core::io::IoStreams;
use crate::core::repo::RepoId;
use crate::core::traits::{Browser, ConfigProvider, GitClient, Prompter, Transport};

/// The `config.toml` key under which `bb repo set-default` stores the chosen
/// repository for `dir` (the per-directory default consulted by [`Context::base_repo`]).
#[must_use]
pub fn default_repo_key(dir: &Path) -> String {
    format!("default_repo:{}", dir.display())
}

/// Holds the injected seam-trait objects so every command is testable by
/// swapping implementations. Cheap to [`Clone`] (everything is `Arc`).
#[derive(Clone)]
pub struct Context {
    pub io: Arc<IoStreams>,
    pub prompter: Arc<dyn Prompter>,
    pub browser: Arc<dyn Browser>,
    pub git: Arc<dyn GitClient>,
    pub config: Arc<dyn ConfigProvider>,
    pub transport: Arc<dyn Transport>,
    pub app_version: String,
    /// Set by `-R/--repo`; overrides git-remote resolution.
    pub repo_override: Option<RepoId>,
}

impl Context {
    /// Resolve the repository the command operates on: the `--repo` override if
    /// set, otherwise the best-priority Bitbucket git remote.
    ///
    /// # Errors
    /// Returns [`GitError`] if remotes cannot be read or none are Bitbucket
    /// repositories.
    pub fn base_repo(&self) -> Result<RepoId, GitError> {
        if let Some(r) = &self.repo_override {
            return Ok(r.clone());
        }
        if let Some(r) = self.configured_default_repo() {
            return Ok(r);
        }
        let remotes = self.git.remotes().map_err(translate_not_a_repo)?;
        remotes.into_iter().next().map(|r| r.repo).ok_or_else(|| {
            GitError::Other("no Bitbucket git remote found; pass --repo WORKSPACE/SLUG".to_owned())
        })
    }

    /// The per-directory default repository persisted by `bb repo set-default`
    /// for the current working directory, if set and parseable. Read from
    /// [`ConfigProvider`] (no git shell-out), so an empty/garbage value simply
    /// falls through to git-remote resolution.
    fn configured_default_repo(&self) -> Option<RepoId> {
        let dir = std::env::current_dir().ok()?;
        let value = self.config.get("", &default_repo_key(&dir))?;
        value.parse().ok()
    }

    /// The host the command targets (the override's host, else the config
    /// default).
    #[must_use]
    pub fn host(&self) -> String {
        self.repo_override
            .as_ref()
            .map_or_else(|| self.config.default_host(), |r| r.host().to_owned())
    }
}

/// Turn the "not a git repository" failure (git exits 128 with that phrase on
/// its stderr when run outside a working tree) into the same actionable message
/// the no-Bitbucket-remote branch uses, so a user outside a repo gets a clear
/// hint instead of a leaked `fatal: not a git repository` line. Any other git
/// failure is passed through untouched.
fn translate_not_a_repo(err: GitError) -> GitError {
    match &err {
        GitError::Command { stderr, .. } if stderr.contains("not a git repository") => {
            GitError::Other(
                "not in a Bitbucket git repository; pass --repo WORKSPACE/SLUG".to_owned(),
            )
        }
        _ => err,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::FileConfig;
    use crate::core::{Commit, Remote};

    /// A [`GitClient`] whose `remotes()` returns a pre-set error; every other
    /// method is unused by [`Context::base_repo`]. `StubRunner` can't set git
    /// stderr (it always returns empty), so we fake the error directly here to
    /// exercise the not-a-repo translation.
    struct FailingGit(fn() -> GitError);

    impl GitClient for FailingGit {
        fn remotes(&self) -> Result<Vec<Remote>, GitError> {
            Err((self.0)())
        }
        fn current_branch(&self) -> Result<String, GitError> {
            unimplemented!()
        }
        fn push(&self, _remote: &str, _refspec: &str) -> Result<(), GitError> {
            unimplemented!()
        }
        fn commits_between(&self, _base: &str, _head: &str) -> Result<Vec<Commit>, GitError> {
            unimplemented!()
        }
        fn fetch(&self, _remote: &str, _refspec: &str) -> Result<(), GitError> {
            unimplemented!()
        }
        fn merge_ff(&self, _committish: &str) -> Result<(), GitError> {
            unimplemented!()
        }
        fn checkout(&self, _branch: &str) -> Result<(), GitError> {
            unimplemented!()
        }
        fn add_remote(&self, _name: &str, _url: &str) -> Result<(), GitError> {
            unimplemented!()
        }
        fn clone_repo(&self, _url: &str, _dir: Option<&str>) -> Result<(), GitError> {
            unimplemented!()
        }
        fn config_set_global(&self, _key: &str, _value: &str) -> Result<(), GitError> {
            unimplemented!()
        }
        fn config_add_global(&self, _key: &str, _value: &str) -> Result<(), GitError> {
            unimplemented!()
        }
    }

    fn ctx_with_git(git: Arc<dyn GitClient>) -> Context {
        let (io, _bufs) = IoStreams::test();
        Context {
            io: Arc::new(io),
            prompter: Arc::new(crate::testsupport::ScriptedPrompter::new()),
            browser: Arc::new(crate::testsupport::RecordingBrowser::default()),
            git,
            config: Arc::new(FileConfig::blank()),
            transport: Arc::new(crate::api::testing::FakeTransport::new()),
            app_version: "test".to_owned(),
            repo_override: None,
        }
    }

    #[test]
    fn base_repo_outside_a_git_repo_gives_friendly_message() {
        // git exits 128 with this stderr when run outside a working tree.
        let git = Arc::new(FailingGit(|| GitError::Command {
            code: 128,
            stderr: "fatal: not a git repository (or any of the parent directories): .git"
                .to_owned(),
        }));
        let err = ctx_with_git(git).base_repo().unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("not in a Bitbucket git repository"),
            "want friendly hint, got: {msg}"
        );
        assert!(msg.contains("--repo WORKSPACE/SLUG"), "got: {msg}");
        // The raw git fatal must not leak through.
        assert!(!msg.contains("fatal:"), "raw git error leaked: {msg}");
    }

    #[test]
    fn base_repo_preserves_unexpected_git_errors() {
        // A genuine, unrelated git failure must propagate verbatim, not be
        // swallowed into the not-a-repo hint.
        let git = Arc::new(FailingGit(|| GitError::Command {
            code: 1,
            stderr: "error: could not read config file".to_owned(),
        }));
        let err = ctx_with_git(git).base_repo().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("could not read config file"), "got: {msg}");
        assert!(
            !msg.contains("not in a Bitbucket git repository"),
            "unexpected error wrongly translated: {msg}"
        );
    }
}
