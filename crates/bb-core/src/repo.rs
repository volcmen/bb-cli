//! [`RepoId`] — a Bitbucket repository identity.

use std::fmt;
use std::str::FromStr;

/// The default Bitbucket Cloud host.
pub const DEFAULT_HOST: &str = "bitbucket.org";

/// A Bitbucket repository identity: `workspace` + `slug` on a `host`.
///
/// This is the Bitbucket Cloud analog of `gh`'s owner/name/host. (Bitbucket Data
/// Center uses `projectKey/repoSlug`; that generalization is deferred to a later
/// epic behind a host abstraction.)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RepoId {
    host: String,
    workspace: String,
    slug: String,
}

impl RepoId {
    /// Construct a repo on the default host ([`DEFAULT_HOST`]).
    pub fn new(workspace: impl Into<String>, slug: impl Into<String>) -> Self {
        Self::with_host(DEFAULT_HOST, workspace, slug)
    }

    /// Construct a repo on an explicit host.
    pub fn with_host(
        host: impl Into<String>,
        workspace: impl Into<String>,
        slug: impl Into<String>,
    ) -> Self {
        Self {
            host: host.into(),
            workspace: workspace.into(),
            slug: slug.into(),
        }
    }

    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    #[must_use]
    pub fn workspace(&self) -> &str {
        &self.workspace
    }

    #[must_use]
    pub fn slug(&self) -> &str {
        &self.slug
    }

    /// `"workspace/slug"`.
    #[must_use]
    pub fn full_name(&self) -> String {
        format!("{}/{}", self.workspace, self.slug)
    }
}

impl fmt::Display for RepoId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.workspace, self.slug)
    }
}

/// Parse the `-R/--repo` flag form: `WORKSPACE/SLUG` (or `HOST/WORKSPACE/SLUG`).
impl FromStr for RepoId {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.trim().split('/').filter(|p| !p.is_empty()).collect();
        match parts.as_slice() {
            [ws, slug] => Ok(RepoId::new(*ws, *slug)),
            [host, ws, slug] => Ok(RepoId::with_host(*host, *ws, *slug)),
            _ => Err(format!("expected WORKSPACE/SLUG, got {s:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_workspace_slug() {
        let r: RepoId = "acme/widgets".parse().unwrap();
        assert_eq!(r.workspace(), "acme");
        assert_eq!(r.slug(), "widgets");
        assert_eq!(r.host(), DEFAULT_HOST);
        assert_eq!(r.full_name(), "acme/widgets");
        assert_eq!(r.to_string(), "acme/widgets");
    }

    #[test]
    fn parses_with_host() {
        let r: RepoId = "bb.acme.com/acme/widgets".parse().unwrap();
        assert_eq!(r.host(), "bb.acme.com");
        assert_eq!(r.workspace(), "acme");
        assert_eq!(r.slug(), "widgets");
    }

    #[test]
    fn rejects_bare_slug() {
        assert!("widgets".parse::<RepoId>().is_err());
    }
}
