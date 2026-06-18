# 066 repo: `bb repo set-default` / `bb repo sync`

Fixes #103. Parity with `gh repo set-default` + `gh repo sync`.

## Goal / user story
- **set-default**: in a checkout with several Bitbucket remotes, pin which
  repository `bb` resolves to (so commands work without `-R`). Today resolution is
  purely the best-priority git remote.
- **sync**: fast-forward the current branch of a fork from its upstream.

## Command surface
`bb repo set-default [REPO] [--view] [--unset]`
- `REPO` = `WORKSPACE/SLUG`. Persisted in `config.toml`, keyed by the **current
  directory** (`default_repo:<abs-cwd>`), so it is per-directory.
- No `REPO`: 1 Bitbucket remote → use it; 2+ → prompt to select; 0 → `FlagError`.
- `REPO` given but not among the current Bitbucket remotes → `FlagError` listing
  the known remotes (skipped when there are no remotes).
- `--view` prints the current default (or "no default repository set").
- `--unset` clears it.
- Resolution order in `Context::base_repo`: `-R` override → configured default →
  git remote. Empty/garbage stored value is ignored (falls through to remotes).

`bb repo sync [--source WORKSPACE/SLUG]`
- Fast-forwards the **current branch** from the source's same-named branch.
- Source: `--source` if given; else the fork's `parent` (one `GET` on the current
  repo); no parent and no `--source` → `FlagError` ("not a fork").
- Builds the source clone URL from host + full name honoring `git_protocol`
  (default https), then `git fetch <url> <branch>` + `git merge --ff-only
  FETCH_HEAD`.
- `--source` path needs no API call (pure git); auto-detect needs auth for the
  parent lookup → `AuthError` when unauthenticated.

## Seam / model changes
- `GitClient::merge_ff(&self, committish)` → `git merge --ff-only <committish>`
  (only new git method; `fetch` already exists).
- `Repository.parent: Option<RepoRef>` (reuses `RepoRef.full_name`).
- `Context::base_repo` consults the configured per-dir default (via
  `ConfigProvider`, no git shell-out — existing tests unaffected as a blank config
  returns `None`).

## Test cases (red first)
set-default:
- `set_default_explicit_repo_writes_config` (REPO among remotes → stored, save).
- `set_default_rejects_repo_not_a_remote` → `FlagError`, nothing saved.
- `set_default_no_arg_single_remote_uses_it`.
- `set_default_no_arg_multi_remote_prompts` (ScriptedPrompter select).
- `set_default_view_reports_current` / `set_default_unset_clears`.
- `base_repo_prefers_configured_default_over_remote`.
sync:
- `sync_with_source_fetches_and_ff` (fetch `<url> <branch>` + `merge --ff-only`).
- `sync_autodetects_parent` (GET current repo → parent → fetch/ff).
- `sync_not_a_fork_is_flag_error` (no parent, no --source).
- `sync_autodetect_not_authed_is_auth_error`.

## Out of scope
`--force`/hard-reset sync; syncing a remote fork via API merge-upstream (Bitbucket
has no equivalent endpoint); cross-host defaults.

## Next: spec 067 — #125 `bb auth setup-git/refresh/switch`
