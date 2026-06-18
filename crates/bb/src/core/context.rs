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
        let remotes = self.git.remotes()?;
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
