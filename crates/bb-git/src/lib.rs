//! `bb-git` — git integration.
//!
//! [`ShellGit`] implements [`GitClient`](bb_core::GitClient) by shelling out to
//! `git` through an injectable [`CommandRunner`] (the analog of `gh`'s
//! `internal/run`), so tests can stub git output deterministically.
//! [`parse_remote_url`] maps a Bitbucket remote URL to a
//! [`RepoId`](bb_core::RepoId).

mod runner;
mod url;

pub use runner::{CommandOutput, CommandRunner, RealRunner};
pub use url::parse_remote_url;

#[cfg(any(test, feature = "test-util"))]
pub use runner::StubRunner;

use std::collections::BTreeMap;
use std::sync::Arc;

use bb_core::{Commit, GitClient, GitError, Remote};

/// A [`GitClient`] that shells out to `git`.
pub struct ShellGit {
    runner: Arc<dyn CommandRunner>,
}

impl ShellGit {
    /// Construct with a custom command runner (e.g. a stub in tests).
    #[must_use]
    pub fn new(runner: Arc<dyn CommandRunner>) -> Self {
        Self { runner }
    }

    /// Construct with the real `git` runner.
    #[must_use]
    pub fn system() -> Self {
        Self {
            runner: Arc::new(RealRunner),
        }
    }

    /// Run a git command, returning stdout on exit 0, else a [`GitError`].
    fn run(&self, args: &[&str]) -> Result<String, GitError> {
        let out = self.runner.run(args)?;
        if out.code == 0 {
            Ok(out.stdout)
        } else {
            Err(GitError::Command {
                code: out.code,
                stderr: out.stderr,
            })
        }
    }
}

fn remote_priority(name: &str) -> (u8, String) {
    let rank = match name {
        "origin" => 0,
        "upstream" => 1,
        _ => 2,
    };
    (rank, name.to_owned())
}

impl GitClient for ShellGit {
    fn current_branch(&self) -> Result<String, GitError> {
        Ok(self
            .run(&["rev-parse", "--abbrev-ref", "HEAD"])?
            .trim()
            .to_owned())
    }

    fn remotes(&self) -> Result<Vec<Remote>, GitError> {
        let out = self.run(&["remote", "-v"])?;
        let mut seen: BTreeMap<String, bb_core::RepoId> = BTreeMap::new();
        for line in out.lines() {
            let mut fields = line.split_whitespace();
            let (Some(name), Some(url)) = (fields.next(), fields.next()) else {
                continue;
            };
            if let Some(repo) = parse_remote_url(url) {
                seen.entry(name.to_owned()).or_insert(repo);
            }
        }
        let mut remotes: Vec<Remote> = seen
            .into_iter()
            .map(|(name, repo)| Remote { name, repo })
            .collect();
        remotes.sort_by_key(|r| remote_priority(&r.name));
        Ok(remotes)
    }

    fn push(&self, remote: &str, refspec: &str) -> Result<(), GitError> {
        self.run(&["push", "--set-upstream", remote, refspec])
            .map(|_| ())
    }

    fn commits_between(&self, base: &str, head: &str) -> Result<Vec<Commit>, GitError> {
        let range = format!("{base}..{head}");
        let out = self.run(&["log", "--pretty=format:%H%x1f%s", &range])?;
        let commits = out
            .lines()
            .filter_map(|line| {
                let (sha, title) = line.split_once('\u{1f}')?;
                Some(Commit {
                    sha: sha.to_owned(),
                    title: title.to_owned(),
                })
            })
            .collect();
        Ok(commits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remotes_parsed_and_prioritized() {
        let stub = Arc::new(StubRunner::new());
        stub.register(
            r"^git remote -v$",
            0,
            "upstream\tgit@bitbucket.org:acme/widgets.git (fetch)\n\
             upstream\tgit@bitbucket.org:acme/widgets.git (push)\n\
             origin\thttps://davidd@bitbucket.org/me/widgets.git (fetch)\n\
             origin\thttps://davidd@bitbucket.org/me/widgets.git (push)\n",
        );
        let git = ShellGit::new(stub);
        let remotes = git.remotes().unwrap();
        // origin sorts before upstream
        assert_eq!(remotes[0].name, "origin");
        assert_eq!(remotes[0].repo.full_name(), "me/widgets");
        assert_eq!(remotes[1].name, "upstream");
        assert_eq!(remotes[1].repo.full_name(), "acme/widgets");
    }

    #[test]
    fn current_branch_trims() {
        let stub = Arc::new(StubRunner::new());
        stub.register(r"rev-parse --abbrev-ref HEAD", 0, "feature/x\n");
        let git = ShellGit::new(stub);
        assert_eq!(git.current_branch().unwrap(), "feature/x");
    }
}
