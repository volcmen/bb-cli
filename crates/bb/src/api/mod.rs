//! `bb-api` — the Bitbucket REST API client.
//!
//! Bitbucket Cloud is pure REST (no GraphQL), so this is intentionally simpler
//! than `gh`'s API layer: typed JSON models, a [`BitbucketClient`] over the
//! [`Transport`](crate::core::Transport) seam, and body-based pagination.

// Absorbed from the former `bb-api` crate: full model/client API retained.
#![allow(dead_code)]

pub mod client;
pub mod models;
pub mod transport;

#[cfg(test)]
pub mod testing;

pub use client::BitbucketClient;
pub use models::{Issue, Links, Membership, PullRequest, User};
pub use transport::ReqwestTransport;
