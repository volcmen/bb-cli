//! [`Context`] — the dependency container (the analog of `gh`'s Factory).

use std::sync::Arc;

use crate::error::GitError;
use crate::io::IoStreams;
use crate::repo::RepoId;
use crate::traits::{Browser, ConfigProvider, GitClient, Prompter, Transport};

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
        let remotes = self.git.remotes()?;
        remotes.into_iter().next().map(|r| r.repo).ok_or_else(|| {
            GitError::Other("no Bitbucket git remote found; pass --repo WORKSPACE/SLUG".to_owned())
        })
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
