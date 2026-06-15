//! Command implementations. Each subcommand is a module that turns parsed clap
//! args + a [`Context`](bb_core::Context) into an action, mirroring `gh`'s
//! one-package-per-command layout.

pub mod auth;
pub mod issue;
pub mod pr;
pub mod repo;
