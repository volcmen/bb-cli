# 067 auth: `bb auth setup-git` / `refresh` / `switch`

Fixes #125 (completes #104's `gh auth` parity). Follow-up to #124 (logout/token).

## Goal / user story
The remaining `gh auth` subcommands:
- **setup-git** — configure git to use `bb` as an HTTPS credential helper, so
  `git`/`bb repo clone` over HTTPS authenticate with the stored token (the HTTPS
  half of #92). Needs a companion `bb auth git-credential` helper.
- **refresh** — force an OAuth token refresh now (today refresh is only
  reactive-on-401).
- **switch** — change the active (default) host among the ones you are logged into.

## Command surface
`bb auth setup-git [--hostname H]`
- Writes (global git config), mirroring `gh`:
  - `git config --global credential.https://<host>.helper ""` (reset)
  - `git config --global --add credential.https://<host>.helper "!<bb> auth git-credential"`
- `<bb>` is the current executable path (falls back to `bb`).
- `FlagError` when not logged in to the host.

`bb auth git-credential <operation>` (hidden; git's credential protocol)
- `get`: reads `host=` from stdin attributes (else default host), prints
  `username=…\npassword=<token>\n`. Username is `x-token-auth` for OAuth, else the
  stored `username`. No stored token → prints nothing (git falls back).
- `store`/`erase`/other: no-op.

`bb auth refresh [--hostname H]`
- Requires `auth_type == oauth` with a stored `refresh_token` +
  `oauth_client_id`/`oauth_client_secret`, else `FlagError`.
- POSTs `grant_type=refresh_token` to the token endpoint (reuses
  `auth::post_form`), persists the new access (+ rotated refresh) token, saves.
- `AuthError` when not logged in.

`bb auth switch [--hostname H]`
- `H` given: must be a logged-in host → set `default_host`, save.
- No `H`: 0 hosts → `FlagError`; 1 → `FlagError` ("only one account"); 2+ →
  prompt to select. Account-switching within one host needs multi-cred storage
  (out of scope — see #125 note).

## Seam changes
- `GitClient::config_set_global(key, value)` → `git config --global <k> <v>`.
- `GitClient::config_add_global(key, value)` → `git config --global --add <k> <v>`.

## Test cases (red first)
- setup-git: writes the reset + add for the host; not-logged-in → FlagError.
- git-credential: `get` formats username/password (oauth → x-token-auth; basic →
  username); unknown host with no creds → empty; `store` → no-op.
- refresh: oauth refresh persists the new token; non-oauth → FlagError; not
  logged in → AuthError.
- switch: explicit host sets default; unknown host → FlagError; no-arg multi
  prompts; no-arg single → FlagError.

## Out of scope
Multi-account (same host) storage; non-https credential protocols.

## Next: spec 068 — #112 `bb alias`
