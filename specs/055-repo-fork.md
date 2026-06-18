# 055 repo: `bb repo fork`

Fixes #100.

## Goal / user story
Fork a Bitbucket repository from the CLI (parity with the core of `gh repo
fork`).

## Command surface
`bb repo fork [SOURCE] [--workspace WS] [--name SLUG]`

- `SOURCE` (`ws/slug`) optional; defaults to the current repo (`base_repo`).
- `--workspace` target workspace (default: your own / Bitbucket's default).
- `--name` rename the fork.
- Exit codes: `AuthError`(4); `FlagError`(1) malformed SOURCE.

## Bitbucket endpoint(s)
`POST /2.0/repositories/{ws}/{slug}/forks` with
`{ workspace:{slug}?, name? }` → the forked repo.

## Behavior & edge cases
- Body omits `workspace`/`name` when not provided (forks to the caller's
  default workspace, keeping the slug).
- Prints `✓ Forked {src} → {fork_full_name}` + the fork URL.

## Test cases (red first)
- `fork_current_repo_posts_to_forks`: default source from `repo_override`;
  POST to `.../forks`; prints fork URL.
- `fork_with_workspace_and_name`: body has `workspace.slug` and `name`.
- `fork_explicit_source`.
- `fork_not_authed_is_auth_error`.

## Out of scope
`--clone`, adding an `upstream` remote.

## Next: spec 056 — #101 `bb repo edit` / `rename`
