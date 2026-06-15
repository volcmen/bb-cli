//! The clap command tree and dispatch. New commands (auth, pr, repo, ...) are
//! added here as their epics land.

use clap::{CommandFactory, Parser, Subcommand};

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
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Show version information
    Version,
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
    match cli.command {
        Some(Commands::Version) => {
            println!("bb version {VERSION}");
            Ok(())
        }
        None => {
            let mut cmd = Cli::command();
            cmd.print_help()?;
            println!();
            Ok(())
        }
    }
}
