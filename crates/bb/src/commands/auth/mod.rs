//! `bb auth` — authentication commands.

mod git_credential;
mod login;
mod logout;
mod refresh;
mod setup_git;
mod status;
mod switch;
mod token;

use crate::core::Context;
use clap::{Args, Subcommand};

#[derive(Args, Debug)]
pub struct AuthArgs {
    #[command(subcommand)]
    command: AuthCommands,
}

#[derive(Subcommand, Debug)]
enum AuthCommands {
    /// Log in to a Bitbucket host
    Login(login::LoginArgs),
    /// View authentication status
    Status(status::StatusArgs),
    /// Log out of a Bitbucket host
    Logout(logout::LogoutArgs),
    /// Print the stored token for a host
    Token(token::TokenArgs),
    /// Configure git to authenticate HTTPS with bb
    SetupGit(setup_git::SetupGitArgs),
    /// Force an OAuth token refresh
    Refresh(refresh::RefreshArgs),
    /// Switch the active host among logged-in hosts
    Switch(switch::SwitchArgs),
    /// Git credential helper (used by `bb auth setup-git`)
    #[command(hide = true)]
    GitCredential(git_credential::GitCredentialArgs),
}

/// Dispatch `bb auth <sub>`.
///
/// # Errors
/// Propagates the sub-command's error.
pub fn run(ctx: &Context, args: AuthArgs) -> anyhow::Result<()> {
    match args.command {
        AuthCommands::Login(a) => login::run(ctx, a),
        AuthCommands::Status(a) => status::run(ctx, a),
        AuthCommands::Logout(a) => logout::run(ctx, a),
        AuthCommands::Token(a) => token::run(ctx, a),
        AuthCommands::SetupGit(a) => setup_git::run(ctx, a),
        AuthCommands::Refresh(a) => refresh::run(ctx, a),
        AuthCommands::Switch(a) => switch::run(ctx, a),
        AuthCommands::GitCredential(a) => git_credential::run(ctx, a),
    }
}
