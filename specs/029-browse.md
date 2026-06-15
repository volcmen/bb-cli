# 029 bb browse

## Goal
Open the current repo (or a PR/branch/commit) on bitbucket.org.

## Command surface
`bb browse [PR-NUMBER] [--branch [B]] [--commit SHA] [--settings] [--no-browser]`. Exit 0; 1 FlagError (conflicting targets / bad number).

## Behavior & edge cases
- Resolve repo via `ctx.base_repo()` (no auth/API needed). Base URL `https://{host}/{ws}/{slug}`.
- Target → URL:
  - none → repo home.
  - `PR-NUMBER` (numeric) → `/pull-requests/{n}`.
  - `--branch [B]` → `/src/{B}` (B defaults to `ctx.git.current_branch()`).
  - `--commit SHA` → `/commits/{SHA}`.
  - `--settings` → `/admin`.
- Mutually-exclusive targets → FlagError. `--no-browser` prints the URL; else `ctx.browser.browse(url)` + print "Opening …".

## Tests
repo url; pr number url; branch (explicit + current via git stub); commit; settings; --no-browser prints; conflicting targets → FlagError.

## Next: spec 030 — bb api (#45)
