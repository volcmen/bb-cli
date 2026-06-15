//! The clap command tree and dispatch. New commands are added to [`Commands`]
//! and routed in [`dispatch`].

use bb_core::{FlagError, RepoId};
use clap::{CommandFactory, Parser, Subcommand};

use crate::commands::{auth::AuthArgs, pr::PrArgs};
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
    #[arg(short = 'R', long = "repo", global = true, value_name = "WORKSPACE/SLUG")]
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
        None => {
            let mut cmd = Cli::command();
            cmd.print_help()?;
            println!();
            Ok(())
        }
    }
}
