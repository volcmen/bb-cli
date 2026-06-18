//! The clap command tree and dispatch. New commands are added to [`Commands`]
//! and routed in [`dispatch`].

use crate::core::{FlagError, RepoId};
use clap::{CommandFactory, Parser, Subcommand};

use crate::commands::{
    api::ApiArgs, auth::AuthArgs, browse::BrowseArgs, completion::CompletionArgs,
    config::ConfigArgs, issue::IssueArgs, man::ManArgs, pipeline::PipelineArgs, pr::PrArgs,
    repo::RepoArgs, search::SearchArgs, ssh_key::SshKeyArgs, variable::VariableArgs,
};
use crate::factory;

/// Full version string: `X.Y.Z (sha date)` (sha/date injected by `build.rs`).
pub const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("BB_BUILD_SHA"),
    " ",
    env!("BB_BUILD_DATE"),
    ")"
);

#[derive(Parser, Debug)]
#[command(
    name = "bb",
    version = VERSION,
    about = "bb — a Bitbucket CLI (a gh for Bitbucket)",
    propagate_version = true
)]
pub struct Cli {
    /// Select another repository as `WORKSPACE/SLUG`
    #[arg(
        short = 'R',
        long = "repo",
        global = true,
        value_name = "WORKSPACE/SLUG"
    )]
    repo: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Show version information
    Version,
    /// Authenticate bb with a Bitbucket host
    Auth(AuthArgs),
    /// Manage pull requests
    Pr(PrArgs),
    /// Work with repositories
    Repo(RepoArgs),
    /// Manage issues
    Issue(IssueArgs),
    /// View CI pipelines
    Pipeline(PipelineArgs),
    /// Open a repository or pull request in the browser
    Browse(BrowseArgs),
    /// Make an authenticated Bitbucket API request
    Api(ApiArgs),
    /// Generate shell completion scripts
    Completion(CompletionArgs),
    /// Generate man pages for bb and its subcommands
    Man(ManArgs),
    /// Get or set local configuration
    Config(ConfigArgs),
    /// Manage your account's SSH keys
    SshKey(SshKeyArgs),
    /// Search repositories, code, and pull requests
    Search(SearchArgs),
    /// Manage Pipelines variables
    Variable(VariableArgs),
}

/// Parse process arguments (auto-exits on `--version`/`--help`/parse errors).
#[must_use]
pub fn parse() -> Cli {
    Cli::parse()
}

/// Run the matched command.
///
/// # Errors
/// Returns the command's error for the caller to classify into an exit code.
pub fn dispatch(cli: Cli) -> anyhow::Result<()> {
    let repo_override = match cli.repo.as_deref() {
        Some(s) => Some(s.parse::<RepoId>().map_err(FlagError::new)?),
        None => None,
    };

    match cli.command {
        Some(Commands::Version) => {
            println!("bb version {VERSION}");
            Ok(())
        }
        Some(Commands::Auth(args)) => {
            let ctx = factory::build_context(repo_override)?;
            crate::commands::auth::run(&ctx, args)
        }
        Some(Commands::Pr(args)) => {
            let ctx = factory::build_context(repo_override)?;
            crate::commands::pr::run(&ctx, args)
        }
        Some(Commands::Repo(args)) => {
            let ctx = factory::build_context(repo_override)?;
            crate::commands::repo::run(&ctx, args)
        }
        Some(Commands::Issue(args)) => {
            let ctx = factory::build_context(repo_override)?;
            crate::commands::issue::run(&ctx, args)
        }
        Some(Commands::Pipeline(args)) => {
            let ctx = factory::build_context(repo_override)?;
            crate::commands::pipeline::run(&ctx, args)
        }
        Some(Commands::Browse(args)) => {
            let ctx = factory::build_context(repo_override)?;
            crate::commands::browse::run(&ctx, args)
        }
        Some(Commands::Api(args)) => {
            let ctx = factory::build_context(repo_override)?;
            crate::commands::api::run(&ctx, args)
        }
        Some(Commands::Completion(args)) => {
            let ctx = factory::build_context(repo_override)?;
            crate::commands::completion::run(&ctx, args)
        }
        Some(Commands::Man(args)) => {
            let ctx = factory::build_context(repo_override)?;
            crate::commands::man::run(&ctx, args)
        }
        Some(Commands::Config(args)) => {
            let ctx = factory::build_context(repo_override)?;
            crate::commands::config::run(&ctx, args)
        }
        Some(Commands::SshKey(args)) => {
            let ctx = factory::build_context(repo_override)?;
            crate::commands::ssh_key::run(&ctx, args)
        }
        Some(Commands::Search(args)) => {
            let ctx = factory::build_context(repo_override)?;
            crate::commands::search::run(&ctx, args)
        }
        Some(Commands::Variable(args)) => {
            let ctx = factory::build_context(repo_override)?;
            crate::commands::variable::run(&ctx, args)
        }

        None => {
            let mut cmd = Cli::command();
            cmd.print_help()?;
            println!();
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `repo view` has a positional repository; the global `-R/--repo` must
    /// still parse *after* the subcommand. Regression for the clap id collision
    /// (positional id `repo` vs the global `repo`) that made `-R` "unexpected".
    #[test]
    fn global_repo_flag_parses_after_repo_view() {
        let cli = Cli::try_parse_from(["bb", "repo", "view", "-R", "acme/widgets"])
            .expect("`-R` should parse after `repo view`");
        assert_eq!(cli.repo.as_deref(), Some("acme/widgets"));
    }

    /// The `repo view` positional still works on its own.
    #[test]
    fn repo_view_positional_parses() {
        Cli::try_parse_from(["bb", "repo", "view", "acme/widgets"])
            .expect("positional WORKSPACE/SLUG should parse");
    }

    /// `-R` is accepted across the other command families too (sanity).
    #[test]
    fn global_repo_flag_parses_after_pr_and_clone() {
        Cli::try_parse_from(["bb", "pr", "list", "-R", "acme/widgets"]).expect("pr list -R");
        Cli::try_parse_from(["bb", "repo", "clone", "-R", "acme/widgets", "acme/widgets"])
            .expect("repo clone -R");
    }

    /// `completion -s <shell>` parses a known shell and rejects an unknown one.
    #[test]
    fn completion_shell_value_parses_and_validates() {
        Cli::try_parse_from(["bb", "completion", "-s", "fish"]).expect("known shell parses");
        Cli::try_parse_from(["bb", "completion"]).expect("shell is optional at parse time");
        assert!(
            Cli::try_parse_from(["bb", "completion", "-s", "tcsh"]).is_err(),
            "unknown shell should be a parse error"
        );
    }

    /// `man -o <dir>` parses; the output directory is required.
    #[test]
    fn man_output_is_required() {
        Cli::try_parse_from(["bb", "man", "-o", "/tmp/bb-man"]).expect("man -o parses");
        assert!(
            Cli::try_parse_from(["bb", "man"]).is_err(),
            "--output is required"
        );
    }
}
