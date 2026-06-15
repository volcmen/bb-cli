//! Bitbucket remote-URL parsing → [`RepoId`].

use bb_core::RepoId;

/// Parse a git remote URL into a Bitbucket [`RepoId`].
///
/// Handles the SSH scp-like (`git@bitbucket.org:ws/slug.git`), `ssh://`, and
/// `https://[user@]` forms, stripping any userinfo and trailing `.git`. Returns
/// `None` for non-Bitbucket hosts or unparseable URLs.
#[must_use]
pub fn parse_remote_url(raw: &str) -> Option<RepoId> {
    let raw = raw.trim();

    let (host, path) = if let Some(rest) = raw.strip_prefix("git@") {
        // scp-like: host:workspace/slug
        let (host, path) = rest.split_once(':')?;
        (host.to_owned(), path.to_owned())
    } else if let Some(rest) = raw.strip_prefix("ssh://") {
        let rest = strip_userinfo(rest);
        let (hostport, path) = rest.split_once('/')?;
        let host = hostport.split(':').next().unwrap_or(hostport).to_owned();
        (host, path.to_owned())
    } else if let Some(rest) = raw
        .strip_prefix("https://")
        .or_else(|| raw.strip_prefix("http://"))
    {
        let rest = strip_userinfo(rest);
        let (host, path) = rest.split_once('/')?;
        (host.to_owned(), path.to_owned())
    } else {
        return None;
    };

    if !is_bitbucket_host(&host) {
        return None;
    }

    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let mut parts = path.split('/').filter(|p| !p.is_empty());
    let workspace = parts.next()?;
    let slug = parts.next()?;

    Some(RepoId::with_host(host, workspace, slug))
}

fn strip_userinfo(rest: &str) -> &str {
    // Drop "user@" before the host, but only within the authority (before '/').
    match (rest.find('@'), rest.find('/')) {
        (Some(at), Some(slash)) if at < slash => &rest[at + 1..],
        (Some(at), None) => &rest[at + 1..],
        _ => rest,
    }
}

fn is_bitbucket_host(host: &str) -> bool {
    let h = host.to_ascii_lowercase();
    h == "bitbucket.org" || h.contains("bitbucket")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scp_ssh() {
        let r = parse_remote_url("git@bitbucket.org:acme/widgets.git").unwrap();
        assert_eq!(r.host(), "bitbucket.org");
        assert_eq!(r.full_name(), "acme/widgets");
    }

    #[test]
    fn parses_ssh_scheme_with_port() {
        let r = parse_remote_url("ssh://git@bitbucket.org:22/acme/widgets.git").unwrap();
        assert_eq!(r.full_name(), "acme/widgets");
    }

    #[test]
    fn parses_https_with_userinfo() {
        let r = parse_remote_url("https://davidd@bitbucket.org/acme/widgets.git").unwrap();
        assert_eq!(r.host(), "bitbucket.org");
        assert_eq!(r.full_name(), "acme/widgets");
    }

    #[test]
    fn parses_https_no_git_suffix() {
        let r = parse_remote_url("https://bitbucket.org/acme/widgets").unwrap();
        assert_eq!(r.full_name(), "acme/widgets");
    }

    #[test]
    fn rejects_non_bitbucket() {
        assert!(parse_remote_url("git@github.com:acme/widgets.git").is_none());
    }
}
