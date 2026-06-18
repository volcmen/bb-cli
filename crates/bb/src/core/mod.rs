//! `bb-core` — the kernel of bb-cli.
//!
//! Holds the dependency-injection **seam traits** ([`Transport`], [`GitClient`],
//! [`Prompter`], [`Browser`], [`ConfigProvider`]), the shared **data types**
//! ([`RepoId`], [`HttpRequest`], [`HttpResponse`]), the terminal **IO layer**
//! ([`IoStreams`], [`ColorScheme`]), the **error/exit-code** model, and the
//! [`Context`] container that wires everything together.
//!
//! Concrete implementations of the seam traits live in the leaf crates
//! (`bb-api`, `bb-config`, `bb-git`) and the `bb` binary. This mirrors the
//! Factory pattern in GitHub's `gh` CLI: commands depend only on the traits, so
//! every command is testable by swapping in fakes.

// Absorbed from the former `bb-core` crate: the full seam-trait + type API is
// retained even though the single binary doesn't exercise every method/field.
#![allow(dead_code)]

pub mod context;
pub mod error;
pub mod http;
pub mod io;
pub mod repo;
pub mod traits;

pub use context::{default_repo_key, Context};
pub use error::{
    ApiError, ApiErrorItem, AuthError, CancelError, ConfigError, ExitCode, FlagError, GitError,
    PromptError, SilentError,
};
pub use http::{HttpRequest, HttpResponse, Method};
pub use io::{ColorScheme, IoStreams};
// TestBuffers is only referenced by tests; gate the re-export so it isn't
// flagged as unused in a non-test build.
#[cfg(test)]
pub use io::TestBuffers;
pub use repo::{RepoId, DEFAULT_HOST};
pub use traits::{Browser, Commit, ConfigProvider, GitClient, Prompter, Remote, Transport};
