//! `bb completion` — generate shell completion scripts (the `gh completion`
//! analog). Pure and offline: it renders the static `clap` command tree, with
//! no network or authentication.

use crate::core::{Context, FlagError};
use clap::{Args, CommandFactory};
use clap_complete::Shell;

#[derive(Args, Debug)]
pub struct CompletionArgs {
    /// Shell to generate a completion script for
    #[arg(short = 's', long = "shell", value_name = "SHELL")]
    pub shell: Option<Shell>,
}

/// Run `bb completion`.
///
/// # Errors
/// Returns [`FlagError`] (exit 1) when `--shell` is omitted while stdout is an
/// interactive terminal — so we never dump a completion script into the user's
/// shell by accident.
pub fn run(ctx: &Context, args: CompletionArgs) -> anyhow::Result<()> {
    let shell = match args.shell {
        Some(s) => s,
        None => {
            // Mirror gh: default to bash only when piped (e.g. `eval "$(...)"`);
            // require an explicit choice on an interactive terminal.
            if ctx.io.is_stdout_tty() {
                return Err(FlagError::new(
                    "the value for `--shell` is required; e.g. `bb completion --shell bash`",
                )
                .into());
            }
            Shell::Bash
        }
    };

    ctx.io.print(&render(shell));
    Ok(())
}

/// Render the completion script for `shell` from the full `bb` command tree.
fn render(shell: Shell) -> String {
    let mut cmd = crate::cli::Cli::command();
    let mut buf = Vec::new();
    clap_complete::generate(shell, &mut cmd, "bb", &mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::api::testing::FakeTransport;
    use crate::config::FileConfig;
    use crate::core::{ConfigProvider, GitClient, TestBuffers, Transport};
    use crate::git::{ShellGit, StubRunner};

    use super::*;
    use crate::testsupport::{test_context, ScriptedPrompter};

    /// A test context wired with inert fakes; `tty` controls `is_stdout_tty()`.
    fn ctx(tty: bool) -> (Context, TestBuffers) {
        let transport: Arc<dyn Transport> = Arc::new(FakeTransport::new());
        let git: Arc<dyn GitClient> = Arc::new(ShellGit::new(Arc::new(StubRunner::new())));
        let config: Arc<dyn ConfigProvider> = Arc::new(FileConfig::blank());
        let prompter = Arc::new(ScriptedPrompter::new());
        test_context(transport, git, config, prompter, tty)
    }

    #[test]
    fn render_bash_emits_script_for_bb() {
        let script = render(Shell::Bash);
        assert!(!script.is_empty());
        assert!(script.contains("bb"), "script should reference the binary");
    }

    #[test]
    fn non_tty_without_shell_defaults_to_bash() {
        let (ctx, bufs) = ctx(false);
        run(&ctx, CompletionArgs { shell: None }).unwrap();
        let out = bufs.stdout_string();
        assert!(!out.is_empty(), "should print a default (bash) script");
        assert!(out.contains("bb"));
    }

    #[test]
    fn tty_without_shell_is_flag_error() {
        let (ctx, _bufs) = ctx(true);
        let err = run(&ctx, CompletionArgs { shell: None }).unwrap_err();
        let flag = err.downcast_ref::<FlagError>().expect("FlagError");
        assert!(flag.to_string().contains("--shell"), "got: {flag}");
    }

    #[test]
    fn explicit_shell_overrides_tty_guard() {
        let (ctx, bufs) = ctx(true);
        run(
            &ctx,
            CompletionArgs {
                shell: Some(Shell::Zsh),
            },
        )
        .unwrap();
        let out = bufs.stdout_string();
        assert!(!out.is_empty());
        // zsh completion scripts start with a `#compdef` directive.
        assert!(
            out.contains("#compdef"),
            "expected a zsh script, got: {out}"
        );
    }
}
