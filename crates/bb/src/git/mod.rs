//! `bb-git` — git integration.
//!
//! [`ShellGit`] implements [`GitClient`](crate::core::GitClient) by shelling out to
//! `git` through an injectable [`CommandRunner`] (the analog of `gh`'s
//! `internal/run`), so tests can stub git output deterministically.
//! [`parse_remote_url`] maps a Bitbucket remote URL to a
//! [`RepoId`](crate::core::RepoId).

// Absorbed from the former `bb-git` crate: full GitClient API retained.
#![allow(dead_code)]

mod runner;
mod url;

pub use runner::{CommandRunner, RealRunner};
pub use url::parse_remote_url;

#[cfg(test)]
pub use runner::StubRunner;

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::core::{Commit, GitClient, GitError, Remote};

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
        let mut seen: BTreeMap<String, crate::core::RepoId> = BTreeMap::new();
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

    fn fetch(&self, remote: &str, refspec: &str) -> Result<(), GitError> {
        self.run(&["fetch", remote, refspec]).map(|_| ())
    }

    fn merge_ff(&self, committish: &str) -> Result<(), GitError> {
        self.run(&["merge", "--ff-only", committish]).map(|_| ())
    }

    fn checkout(&self, branch: &str) -> Result<(), GitError> {
        self.run(&["checkout", branch]).map(|_| ())
    }

    fn add_remote(&self, name: &str, url: &str) -> Result<(), GitError> {
        self.run(&["remote", "add", name, url]).map(|_| ())
    }

    fn clone_repo(&self, url: &str, dir: Option<&str>) -> Result<(), GitError> {
        // `--` stops git option parsing so a url/dir starting with `-` can't be
        // interpreted as a flag (e.g. `--upload-pack=...`).
        let mut args = vec!["clone", "--", url];
        if let Some(d) = dir {
            args.push(d);
        }
        self.run(&args).map(|_| ())
    }

    fn config_set_global(&self, key: &str, value: &str) -> Result<(), GitError> {
        self.run(&["config", "--global", key, value]).map(|_| ())
    }

    fn config_add_global(&self, key: &str, value: &str) -> Result<(), GitError> {
        self.run(&["config", "--global", "--add", key, value])
            .map(|_| ())
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

    #[test]
    fn current_branch_trims_surrounding_whitespace() {
        let stub = Arc::new(StubRunner::new());
        stub.register(r"rev-parse --abbrev-ref HEAD", 0, "  main \n");
        let git = ShellGit::new(stub);
        assert_eq!(git.current_branch().unwrap(), "main");
    }

    #[test]
    fn remotes_dedup_fetch_and_push_into_one() {
        let stub = Arc::new(StubRunner::new());
        // origin appears twice (fetch + push) — must collapse to a single Remote.
        stub.register(
            r"^git remote -v$",
            0,
            "origin\tgit@bitbucket.org:acme/widgets.git (fetch)\n\
             origin\tgit@bitbucket.org:acme/widgets.git (push)\n",
        );
        let git = ShellGit::new(stub);
        let remotes = git.remotes().unwrap();
        assert_eq!(remotes.len(), 1);
        assert_eq!(remotes[0].name, "origin");
        assert_eq!(remotes[0].repo.full_name(), "acme/widgets");
    }

    #[test]
    fn remotes_priority_sorts_origin_upstream_other() {
        let stub = Arc::new(StubRunner::new());
        // Registered out of priority order; result must be origin < upstream < other.
        stub.register(
            r"^git remote -v$",
            0,
            "fork\tgit@bitbucket.org:me/widgets.git (fetch)\n\
             upstream\tgit@bitbucket.org:acme/widgets.git (fetch)\n\
             origin\tgit@bitbucket.org:team/widgets.git (fetch)\n",
        );
        let git = ShellGit::new(stub);
        let names: Vec<String> = git.remotes().unwrap().into_iter().map(|r| r.name).collect();
        assert_eq!(names, vec!["origin", "upstream", "fork"]);
    }

    #[test]
    fn remotes_skip_non_bitbucket() {
        let stub = Arc::new(StubRunner::new());
        stub.register(
            r"^git remote -v$",
            0,
            "gh\tgit@github.com:acme/widgets.git (fetch)\n\
             gh\tgit@github.com:acme/widgets.git (push)\n\
             origin\tgit@bitbucket.org:acme/widgets.git (fetch)\n\
             origin\tgit@bitbucket.org:acme/widgets.git (push)\n",
        );
        let git = ShellGit::new(stub);
        let remotes = git.remotes().unwrap();
        assert_eq!(remotes.len(), 1);
        assert_eq!(remotes[0].name, "origin");
    }

    #[test]
    fn remotes_empty_output_is_empty_vec() {
        let stub = Arc::new(StubRunner::new());
        stub.register(r"^git remote -v$", 0, "");
        let git = ShellGit::new(stub);
        assert!(git.remotes().unwrap().is_empty());
    }

    #[test]
    fn remotes_propagates_command_failure() {
        let stub = Arc::new(StubRunner::new());
        stub.register(r"^git remote -v$", 128, "");
        let git = ShellGit::new(stub);
        let err = git.remotes().unwrap_err();
        assert!(matches!(err, GitError::Command { code: 128, .. }));
    }

    #[test]
    fn commits_between_parses_sha_and_title() {
        let stub = Arc::new(StubRunner::new());
        // git log emits "%H\x1f%s" per line, newest first.
        stub.register(
            r"^git log",
            0,
            "abc123\u{1f}Add feature\ndef456\u{1f}Fix bug\n",
        );
        let git = ShellGit::new(stub);
        let commits = git.commits_between("main", "feature/x").unwrap();
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].sha, "abc123");
        assert_eq!(commits[0].title, "Add feature");
        assert_eq!(commits[1].sha, "def456");
        assert_eq!(commits[1].title, "Fix bug");
    }

    #[test]
    fn commits_between_handles_empty() {
        let stub = Arc::new(StubRunner::new());
        stub.register(r"^git log", 0, "");
        let git = ShellGit::new(stub);
        assert!(git.commits_between("main", "main").unwrap().is_empty());
    }

    #[test]
    fn commits_between_skips_malformed_lines() {
        let stub = Arc::new(StubRunner::new());
        // A line without the \x1f separator must be skipped, not panic.
        stub.register(
            r"^git log",
            0,
            "abc123\u{1f}Good\nno-separator-here\ndef456\u{1f}Also good\n",
        );
        let git = ShellGit::new(stub);
        let commits = git.commits_between("main", "head").unwrap();
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].title, "Good");
        assert_eq!(commits[1].title, "Also good");
    }

    #[test]
    fn commits_between_preserves_titles_with_special_chars() {
        let stub = Arc::new(StubRunner::new());
        stub.register(
            r"^git log",
            0,
            "abc123\u{1f}feat: add x (with: colons, commas)\n",
        );
        let git = ShellGit::new(stub);
        let commits = git.commits_between("main", "head").unwrap();
        assert_eq!(commits[0].title, "feat: add x (with: colons, commas)");
    }

    #[test]
    fn push_issues_set_upstream_args() {
        let stub = Arc::new(StubRunner::new());
        // Assert the exact argument shape: push --set-upstream <remote> <refspec>.
        stub.register(r"^git push --set-upstream origin feature/x$", 0, "");
        let git = ShellGit::new(stub);
        git.push("origin", "feature/x").unwrap();
    }

    #[test]
    fn push_propagates_command_failure() {
        let stub = Arc::new(StubRunner::new());
        stub.register(r"^git push --set-upstream origin feature/x$", 1, "");
        let git = ShellGit::new(stub);
        let err = git.push("origin", "feature/x").unwrap_err();
        assert!(matches!(err, GitError::Command { code: 1, .. }));
    }
}
