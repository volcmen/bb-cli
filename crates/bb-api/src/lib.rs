//! `bb-api` — the Bitbucket REST API client.
//!
//! Bitbucket Cloud is pure REST (no GraphQL), so this is intentionally simpler
//! than `gh`'s API layer: typed JSON models, a [`BitbucketClient`] over the
//! [`Transport`](bb_core::Transport) seam, and body-based pagination.

pub mod client;
pub mod models;
pub mod transport;

#[cfg(any(test, feature = "test-util"))]
pub mod testing;

pub use client::BitbucketClient;
pub use models::{
    Branch, BranchRef, CloneLink, Link, Links, Participant, PullRequest, Rendered, RepoLinks,
    RepoRef, Repository, User,
};
pub use transport::ReqwestTransport;
