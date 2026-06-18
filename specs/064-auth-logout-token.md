# 064 auth: `bb auth logout` / `token`

Addresses #104 (core). `setup-git`, `refresh`, `switch` are split to a follow-up
(they need a git credential-helper write, a forced OAuth refresh, and
multi-account storage respectively).

## Command surface
- `bb auth logout [--hostname H]` — clear stored credentials for the host.
- `bb auth token [--hostname H]` — print the stored token (for scripting).

Host defaults to the configured default host. Exit codes: `FlagError`(1) logout
when not logged in; `AuthError`(4) token when not logged in.

## Bitbucket endpoint(s)
None — local credential store (`ConfigProvider::get`/`unset_host`/`save`).

## Behavior & edge cases
- logout: if no token for the host → `FlagError` ("not logged in to {host}");
  else `unset_host` + `save`, print `✓ Logged out of {host}`.
- token: print the token, or `AuthError` if unset.

## Test cases (red first)
- `logout_clears_host` (temp-backed config; token gone after).
- `logout_not_logged_in_is_flag_error`.
- `token_prints_stored_token`.
- `token_not_logged_in_is_auth_error`.

## Out of scope
`auth setup-git` / `refresh` / `switch` — follow-up issue.

## Next: spec 065 — #107 `bb snippet` (Bitbucket Snippets)
