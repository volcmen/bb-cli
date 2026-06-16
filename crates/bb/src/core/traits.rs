//! Dependency-injection **seam traits**. Concrete implementations live in the
//! leaf crates and the `bb` binary; tests swap in fakes.

use crate::core::error::{ApiError, ConfigError, GitError, PromptError};
use crate::core::http::{HttpRequest, HttpResponse};
use crate::core::repo::RepoId;

/// HTTP transport seam — the entire network-testability story (the analog of
/// `gh`'s `ReplaceTripper`). The real impl wraps `reqwest`; tests inject a fake.
pub trait Transport: Send + Sync {
    fn execute(&self, req: HttpRequest) -> Result<HttpResponse, ApiError>;
}

/// A parsed git remote pointing at a Bitbucket repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    pub name: String,
    pub repo: RepoId,
}

/// A commit (the subset used for PR title/body autofill).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub sha: String,
    pub title: String,
}

/// Git seam — a shell-out wrapper (the analog of `gh`'s `git` client +
/// `internal/run`). Implementations hold an injectable command runner so tests
/// can stub git output.
pub trait GitClient: Send + Sync {
    /// The current branch name (e.g. `feature/x`).
    fn current_branch(&self) -> Result<String, GitError>;
    /// Bitbucket remotes, best-priority first (e.g. `origin` before others).
    fn remotes(&self) -> Result<Vec<Remote>, GitError>;
    /// `git push --set-upstream <remote> <refspec>`.
    fn push(&self, remote: &str, refspec: &str) -> Result<(), GitError>;
    /// Commits in `base..head`, newest first.
    fn commits_between(&self, base: &str, head: &str) -> Result<Vec<Commit>, GitError>;
    /// `git fetch <remote> <refspec>`.
    fn fetch(&self, remote: &str, refspec: &str) -> Result<(), GitError>;
    /// `git checkout <branch>`.
    fn checkout(&self, branch: &str) -> Result<(), GitError>;
    /// `git remote add <name> <url>`.
    fn add_remote(&self, name: &str, url: &str) -> Result<(), GitError>;
    /// `git clone <url> [dir]`.
    fn clone_repo(&self, url: &str, dir: Option<&str>) -> Result<(), GitError>;
}

/// Interactive prompt seam. The real impl uses `inquire`; tests use a scripted
/// fake.
pub trait Prompter: Send + Sync {
    fn confirm(&self, message: &str, default: bool) -> Result<bool, PromptError>;
    fn input(&self, message: &str, default: Option<&str>) -> Result<String, PromptError>;
    fn password(&self, message: &str) -> Result<String, PromptError>;
    fn select(&self, message: &str, options: &[String]) -> Result<usize, PromptError>;
    fn editor(&self, message: &str, initial: &str) -> Result<String, PromptError>;
}

/// Web-browser launcher seam (for `--web` flows).
pub trait Browser: Send + Sync {
    fn browse(&self, url: &str) -> Result<(), std::io::Error>;
}

/// Config seam — host-scoped key/value access plus auth-token retrieval (the
/// analog of `gh`'s `config.Config` + the `envConfig` decorator). `set` takes
/// `&self`; implementations use interior mutability.
pub trait ConfigProvider: Send + Sync {
    fn get(&self, host: &str, key: &str) -> Option<String>;
    fn set(&self, host: &str, key: &str, value: &str) -> Result<(), ConfigError>;
    fn unset_host(&self, host: &str) -> Result<(), ConfigError>;
    fn default_host(&self) -> String;
    fn auth_token(&self, host: &str) -> Option<String>;
    fn hosts(&self) -> Vec<String>;
    /// Persist to disk. In-memory/test implementations may no-op.
    fn save(&self) -> Result<(), ConfigError>;
}
