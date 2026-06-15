//! Typed Bitbucket Cloud API models (the subset needed for Epic 0). These are
//! deliberately lenient (`Option`-heavy) so partial responses deserialize.

use serde::{Deserialize, Serialize};

/// A Bitbucket user/account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub account_id: Option<String>,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub uuid: Option<String>,
}

impl User {
    /// The best available human-facing name.
    #[must_use]
    pub fn label(&self) -> String {
        self.display_name
            .clone()
            .or_else(|| self.username.clone())
            .or_else(|| self.account_id.clone())
            .unwrap_or_else(|| "unknown".to_owned())
    }
}

/// A repository's main branch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MainBranch {
    pub name: String,
}

/// A Bitbucket repository.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repository {
    pub slug: Option<String>,
    pub name: Option<String>,
    pub full_name: Option<String>,
    pub is_private: Option<bool>,
    pub description: Option<String>,
    pub mainbranch: Option<MainBranch>,
    pub links: Option<RepoLinks>,
}

/// A repository's `links` (the subset we use: web URL + clone URLs).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RepoLinks {
    pub html: Option<Link>,
    #[serde(default)]
    pub clone: Vec<CloneLink>,
}

/// One clone URL entry (`{ "name": "https"|"ssh", "href": "..." }`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneLink {
    pub name: Option<String>,
    pub href: String,
}

impl Repository {
    #[must_use]
    pub fn html_url(&self) -> Option<&str> {
        self.links
            .as_ref()
            .and_then(|l| l.html.as_ref())
            .map(|h| h.href.as_str())
    }

    /// The clone URL for `protocol` (`"https"` or `"ssh"`), if present.
    #[must_use]
    pub fn clone_url(&self, protocol: &str) -> Option<&str> {
        self.links
            .as_ref()?
            .clone
            .iter()
            .find(|c| c.name.as_deref() == Some(protocol))
            .map(|c| c.href.as_str())
    }
}

/// A branch name wrapper (`{ "name": "..." }`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Branch {
    pub name: String,
}

/// A PR source/destination endpoint
/// (`{ "branch": { "name": ... }, "repository": { "full_name": ... } }`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BranchRef {
    pub branch: Option<Branch>,
    pub repository: Option<RepoRef>,
}

impl BranchRef {
    #[must_use]
    pub fn branch_name(&self) -> &str {
        self.branch.as_ref().map_or("", |b| b.name.as_str())
    }

    /// The `full_name` (`workspace/slug`) of this endpoint's repository, if
    /// present. Used to detect a cross-fork PR source.
    #[must_use]
    pub fn repo_full_name(&self) -> Option<&str> {
        self.repository
            .as_ref()
            .and_then(|r| r.full_name.as_deref())
    }
}

/// A repository reference embedded in a PR source/destination.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RepoRef {
    pub full_name: Option<String>,
}

/// A single link (`{ "href": "..." }`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Link {
    pub href: String,
}

/// The `links` object (the subset we use).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Links {
    pub html: Option<Link>,
}

impl Links {
    #[must_use]
    pub fn html_href(&self) -> Option<&str> {
        self.html.as_ref().map(|l| l.href.as_str())
    }
}

/// A pull request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    pub id: u64,
    pub title: Option<String>,
    pub state: Option<String>,
    #[serde(default)]
    pub source: BranchRef,
    #[serde(default)]
    pub destination: BranchRef,
    #[serde(default)]
    pub links: Links,
    pub author: Option<User>,
    pub description: Option<String>,
    pub summary: Option<Rendered>,
    pub close_source_branch: Option<bool>,
    #[serde(default)]
    pub participants: Vec<Participant>,
    #[serde(default)]
    pub reviewers: Vec<User>,
}

/// Rendered content (e.g. `summary.raw`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Rendered {
    pub raw: Option<String>,
}

/// A PR participant with their approval state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Participant {
    pub user: Option<User>,
    pub role: Option<String>,
    #[serde(default)]
    pub approved: bool,
}

impl PullRequest {
    #[must_use]
    pub fn html_url(&self) -> Option<&str> {
        self.links.html_href()
    }

    /// Best available description text (`description`, then `summary.raw`).
    #[must_use]
    pub fn body(&self) -> Option<&str> {
        self.description
            .as_deref()
            .or_else(|| self.summary.as_ref().and_then(|s| s.raw.as_deref()))
    }

    /// Users who have approved this PR.
    #[must_use]
    pub fn approvals(&self) -> Vec<&User> {
        self.participants
            .iter()
            .filter(|p| p.approved)
            .filter_map(|p| p.user.as_ref())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_pull_request() {
        let json = r#"{
            "id": 42,
            "title": "Add widget",
            "state": "OPEN",
            "source": { "branch": { "name": "feature/x" } },
            "destination": { "branch": { "name": "main" } },
            "links": { "html": { "href": "https://bitbucket.org/acme/widgets/pull-requests/42" } }
        }"#;
        let pr: PullRequest = serde_json::from_str(json).unwrap();
        assert_eq!(pr.id, 42);
        assert_eq!(pr.source.branch_name(), "feature/x");
        assert_eq!(pr.destination.branch_name(), "main");
        assert_eq!(
            pr.html_url(),
            Some("https://bitbucket.org/acme/widgets/pull-requests/42")
        );
    }

    #[test]
    fn user_label_prefers_display_name() {
        let u: User = serde_json::from_str(r#"{"username":"d","display_name":"David"}"#).unwrap();
        assert_eq!(u.label(), "David");
    }
}
