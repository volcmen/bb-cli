//! `bb browse` — open a repository, pull request, branch, or commit in the
//! browser.

use crate::core::{Context, FlagError};
use clap::Args;

#[derive(Args, Debug)]
pub struct BrowseArgs {
    /// A pull request number to open (omit to open the repository)
    #[arg(value_name = "PR-NUMBER")]
    pub pr: Option<String>,
    /// Open the source view for a branch (no value = current branch)
    #[arg(long, value_name = "BRANCH", num_args = 0..=1, default_missing_value = "")]
    pub branch: Option<String>,
    /// Open a specific commit
    #[arg(long, value_name = "SHA")]
    pub commit: Option<String>,
    /// Open the repository's settings/admin page
    #[arg(long)]
    pub settings: bool,
    /// Print the destination URL instead of opening a browser
    #[arg(long)]
    pub no_browser: bool,
}

/// Run `bb browse`.
///
/// Resolves the base repo (no auth/API needed) and computes a destination URL
/// from the single chosen target — a PR number, a branch, a commit, the
/// settings page, or (default) the repository home. With `--no-browser` the URL
/// is printed; otherwise it is opened in the browser.
///
/// # Errors
/// Returns [`FlagError`] if more than one target is given, or if the positional
/// pull-request argument is not a valid number. Propagates
/// [`GitError`](crate::core::GitError) from repo / current-branch resolution.
pub fn run(ctx: &Context, args: BrowseArgs) -> anyhow::Result<()> {
    let repo = ctx.base_repo()?;
    let base = format!(
        "https://{}/{}/{}",
        repo.host(),
        repo.workspace(),
        repo.slug()
    );

    // Exactly one of {pr, branch, commit, settings} may select a sub-target.
    let target_count = usize::from(args.pr.is_some())
        + usize::from(args.branch.is_some())
        + usize::from(args.commit.is_some())
        + usize::from(args.settings);
    if target_count > 1 {
        return Err(FlagError::new(
            "specify only one of a pull request number, --branch, --commit, or --settings",
        )
        .into());
    }

    let url = if let Some(pr) = &args.pr {
        let n: u64 = pr
            .trim()
            .parse()
            .map_err(|_| FlagError::new("invalid pull request number"))?;
        format!("{base}/pull-requests/{n}")
    } else if let Some(branch) = &args.branch {
        let branch = if branch.is_empty() {
            ctx.git.current_branch()?
        } else {
            branch.clone()
        };
        format!("{base}/src/{}", encode_branch_path(&branch))
    } else if let Some(sha) = &args.commit {
        format!("{base}/commits/{sha}")
    } else if args.settings {
        format!("{base}/admin")
    } else {
        base
    };

    if args.no_browser {
        ctx.io.println(&url);
    } else {
        ctx.browser.browse(&url)?;
        ctx.io.println(&format!("Opening {url} in your browser."));
    }
    Ok(())
}

/// Percent-encode a branch name for use in a URL path, preserving `/` as a path
/// separator (a branch like `feature/x` becomes `feature/x`, but spaces and
/// other reserved bytes within a segment are `%XX`-encoded).
fn encode_branch_path(branch: &str) -> String {
    branch
        .split('/')
        .map(encode_segment)
        .collect::<Vec<_>>()
        .join("/")
}

/// Minimal percent-encoding of a single path segment: the unreserved set passes
/// through, everything else becomes `%XX`.
fn encode_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::api::testing::FakeTransport;
    use crate::config::FileConfig;
    use crate::core::{ConfigProvider, Context, GitClient, IoStreams, Prompter, RepoId, Transport};
    use crate::git::{ShellGit, StubRunner};

    use super::*;
    use crate::testsupport::{RecordingBrowser, ScriptedPrompter};

    /// Build a `Context` wired to our own `RecordingBrowser` so opened URLs can
    /// be asserted, with `repo_override` set to `acme/widgets`. The git client
    /// is built from `runner` so `--branch` (current) can be stubbed.
    fn ctx_with(
        browser: Arc<RecordingBrowser>,
        runner: Arc<StubRunner>,
    ) -> (Context, crate::core::TestBuffers) {
        let (io, bufs) = IoStreams::test();
        let transport: Arc<dyn Transport> = Arc::new(FakeTransport::new());
        let git: Arc<dyn GitClient> = Arc::new(ShellGit::new(runner));
        let config: Arc<dyn ConfigProvider> = Arc::new(FileConfig::blank());
        let prompter: Arc<dyn Prompter> = Arc::new(ScriptedPrompter::new());
        let ctx = Context {
            io: Arc::new(io),
            prompter,
            browser,
            git,
            config,
            transport,
            app_version: "test".to_owned(),
            repo_override: Some(RepoId::new("acme", "widgets")),
        };
        (ctx, bufs)
    }

    fn args() -> BrowseArgs {
        BrowseArgs {
            pr: None,
            branch: None,
            commit: None,
            settings: false,
            no_browser: false,
        }
    }

    fn opened(browser: &RecordingBrowser) -> String {
        browser.urls.lock().unwrap().last().cloned().expect("a URL")
    }

    #[test]
    fn no_target_opens_repo_home() {
        let browser = Arc::new(RecordingBrowser::default());
        let (ctx, bufs) = ctx_with(browser.clone(), Arc::new(StubRunner::new()));
        run(&ctx, args()).unwrap();
        assert_eq!(opened(&browser), "https://bitbucket.org/acme/widgets");
        assert!(bufs
            .stdout_string()
            .contains("Opening https://bitbucket.org/acme/widgets in your browser."));
    }

    #[test]
    fn pr_number_opens_pull_request() {
        let browser = Arc::new(RecordingBrowser::default());
        let (ctx, _bufs) = ctx_with(browser.clone(), Arc::new(StubRunner::new()));
        run(
            &ctx,
            BrowseArgs {
                pr: Some("42".to_owned()),
                ..args()
            },
        )
        .unwrap();
        assert_eq!(
            opened(&browser),
            "https://bitbucket.org/acme/widgets/pull-requests/42"
        );
    }

    #[test]
    fn non_numeric_pr_is_flag_error() {
        let browser = Arc::new(RecordingBrowser::default());
        let (ctx, _bufs) = ctx_with(browser, Arc::new(StubRunner::new()));
        let err = run(
            &ctx,
            BrowseArgs {
                pr: Some("oops".to_owned()),
                ..args()
            },
        )
        .unwrap_err();
        let flag = err.downcast_ref::<FlagError>().expect("FlagError");
        assert!(flag.to_string().contains("invalid pull request number"));
    }

    #[test]
    fn explicit_branch_opens_src_view() {
        let browser = Arc::new(RecordingBrowser::default());
        let (ctx, _bufs) = ctx_with(browser.clone(), Arc::new(StubRunner::new()));
        run(
            &ctx,
            BrowseArgs {
                branch: Some("feature/x".to_owned()),
                ..args()
            },
        )
        .unwrap();
        assert_eq!(
            opened(&browser),
            "https://bitbucket.org/acme/widgets/src/feature/x"
        );
    }

    #[test]
    fn current_branch_resolved_via_git() {
        let runner = Arc::new(StubRunner::new());
        runner.register(r"rev-parse --abbrev-ref HEAD", 0, "feature/cur\n");
        let browser = Arc::new(RecordingBrowser::default());
        let (ctx, _bufs) = ctx_with(browser.clone(), runner);
        run(
            &ctx,
            BrowseArgs {
                // `--branch` with no value -> default_missing_value = "".
                branch: Some(String::new()),
                ..args()
            },
        )
        .unwrap();
        assert_eq!(
            opened(&browser),
            "https://bitbucket.org/acme/widgets/src/feature/cur"
        );
    }

    #[test]
    fn commit_opens_commit_view() {
        let browser = Arc::new(RecordingBrowser::default());
        let (ctx, _bufs) = ctx_with(browser.clone(), Arc::new(StubRunner::new()));
        run(
            &ctx,
            BrowseArgs {
                commit: Some("abc123".to_owned()),
                ..args()
            },
        )
        .unwrap();
        assert_eq!(
            opened(&browser),
            "https://bitbucket.org/acme/widgets/commits/abc123"
        );
    }

    #[test]
    fn settings_opens_admin() {
        let browser = Arc::new(RecordingBrowser::default());
        let (ctx, _bufs) = ctx_with(browser.clone(), Arc::new(StubRunner::new()));
        run(
            &ctx,
            BrowseArgs {
                settings: true,
                ..args()
            },
        )
        .unwrap();
        assert_eq!(opened(&browser), "https://bitbucket.org/acme/widgets/admin");
    }

    #[test]
    fn no_browser_prints_url_and_does_not_open() {
        let browser = Arc::new(RecordingBrowser::default());
        let (ctx, bufs) = ctx_with(browser.clone(), Arc::new(StubRunner::new()));
        run(
            &ctx,
            BrowseArgs {
                no_browser: true,
                ..args()
            },
        )
        .unwrap();
        assert_eq!(
            bufs.stdout_string().trim(),
            "https://bitbucket.org/acme/widgets"
        );
        assert!(browser.urls.lock().unwrap().is_empty());
    }

    #[test]
    fn two_targets_is_flag_error() {
        let browser = Arc::new(RecordingBrowser::default());
        let (ctx, _bufs) = ctx_with(browser, Arc::new(StubRunner::new()));
        let err = run(
            &ctx,
            BrowseArgs {
                commit: Some("abc123".to_owned()),
                settings: true,
                ..args()
            },
        )
        .unwrap_err();
        let flag = err.downcast_ref::<FlagError>().expect("FlagError");
        assert!(flag.to_string().contains("specify only one of"));
    }
}
