# Security Policy

## Supported versions

`bb` is pre-1.0; security fixes land on the latest published `0.x` release.

| Version | Supported |
|---------|-----------|
| latest `0.x` | ✅ |
| older | ❌ |

## Reporting a vulnerability

**Please do not open a public issue for security vulnerabilities.**

Report privately via GitHub's [private vulnerability reporting](https://github.com/volcmen/bb-cli/security/advisories/new)
("Report a vulnerability" on the repository's **Security** tab). Include:

- a description and the impact,
- steps to reproduce or a proof of concept,
- affected version (`bb --version`) and platform.

You can expect an initial acknowledgement within a few days. Once a fix is ready,
a patched release is published to crates.io and the advisory is disclosed.

## Scope & handling notes

- Credentials are stored in `~/.config/bb/hosts.toml` with `0600` permissions;
  never paste tokens into issues or PRs. Use GitHub's private reporting instead.
- OAuth uses PKCE with a loopback redirect (RFC 8252); `Authorization` headers and
  client secrets are redacted from debug output and never logged.
