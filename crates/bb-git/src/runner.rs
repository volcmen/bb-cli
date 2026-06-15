//! The [`CommandRunner`] seam: a real `git` runner and a regex-driven stub.

use bb_core::GitError;

/// The result of running a command.
#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Runs `git` subcommands. Injected into [`ShellGit`](crate::ShellGit) so tests
/// can stub output.
pub trait CommandRunner: Send + Sync {
    fn run(&self, args: &[&str]) -> Result<CommandOutput, GitError>;
}

/// Shells out to the real `git` binary.
pub struct RealRunner;

impl CommandRunner for RealRunner {
    fn run(&self, args: &[&str]) -> Result<CommandOutput, GitError> {
        let git = which::which("git").map_err(|e| GitError::NotFound(e.to_string()))?;
        let output = std::process::Command::new(git)
            .args(args)
            .output()
            .map_err(|e| GitError::Other(e.to_string()))?;
        Ok(CommandOutput {
            code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

#[cfg(any(test, feature = "test-util"))]
pub use stub::StubRunner;

#[cfg(any(test, feature = "test-util"))]
mod stub {
    use std::sync::Mutex;

    use bb_core::GitError;

    use super::{CommandOutput, CommandRunner};

    struct CmdStub {
        pattern: regex::Regex,
        code: i32,
        stdout: String,
        matched: bool,
    }

    /// A regex-driven git stub (the analog of `gh`'s `run.Stub`). On drop it
    /// asserts every registered stub was matched.
    #[derive(Default)]
    pub struct StubRunner {
        stubs: Mutex<Vec<CmdStub>>,
    }

    impl StubRunner {
        #[must_use]
        pub fn new() -> Self {
            Self::default()
        }

        /// Register a stub: when the joined command line (`git <args>`) matches
        /// `pattern`, return `code` + `stdout`.
        ///
        /// # Panics
        /// Panics if `pattern` is not a valid regex.
        pub fn register(&self, pattern: &str, code: i32, stdout: &str) {
            self.stubs.lock().expect("stubs poisoned").push(CmdStub {
                pattern: regex::Regex::new(pattern).expect("invalid stub regex"),
                code,
                stdout: stdout.to_owned(),
                matched: false,
            });
        }
    }

    impl CommandRunner for StubRunner {
        fn run(&self, args: &[&str]) -> Result<CommandOutput, GitError> {
            let line = format!("git {}", args.join(" "));
            let mut stubs = self.stubs.lock().expect("stubs poisoned");
            for stub in stubs.iter_mut() {
                if !stub.matched && stub.pattern.is_match(&line) {
                    stub.matched = true;
                    return Ok(CommandOutput {
                        code: stub.code,
                        stdout: stub.stdout.clone(),
                        stderr: String::new(),
                    });
                }
            }
            panic!("StubRunner: no stub matched command line: {line:?}");
        }
    }

    impl Drop for StubRunner {
        fn drop(&mut self) {
            if std::thread::panicking() {
                return;
            }
            let stubs = self.stubs.lock().expect("stubs poisoned");
            let unmatched: Vec<&str> = stubs
                .iter()
                .filter(|s| !s.matched)
                .map(|s| s.pattern.as_str())
                .collect();
            assert!(
                unmatched.is_empty(),
                "StubRunner: these git stubs were never matched: {unmatched:?}"
            );
        }
    }
}
