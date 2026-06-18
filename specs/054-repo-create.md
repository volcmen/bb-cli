# 054 repo: `bb repo create`

Fixes #99.

## Goal / user story
Create a new Bitbucket repository from the CLI (parity with the core of `gh repo
create`).

## Command surface
`bb repo create <WORKSPACE/SLUG> [--public] [--description D] [--project KEY]`

- Private by default; `--public` makes it public.
- `--description`, `--project <KEY>` optional.
- Exit codes: `AuthError`(4) unauthenticated; `FlagError`(1) malformed name.

## Bitbucket endpoint(s)
`POST /2.0/repositories/{ws}/{slug}` with
`{ scm:"git", is_private, description?, project:{key}? }` → the created repo.

Note: creating a repo requires the `repository:admin` scope; the default
embedded OAuth consumer lacks it (see #52), so this may return a scope error at
runtime until a broader consumer is registered.

## Behavior & edge cases
- Body always sends `scm:"git"` and `is_private` (= `!--public`).
- `description`/`project` omitted from the body when not given.
- Prints `✓ Created {ws}/{slug}` + the repo web URL.

## Test cases (red first)
- `create_private_by_default`: POST body `is_private==true`, `scm=="git"`,
  description present.
- `create_public_flag`: `is_private==false`.
- `create_with_project`: body `project.key` set.
- `create_invalid_name_is_flag_error` (no HTTP).
- `create_not_authed_is_auth_error`.

## Out of scope
`--clone` (would duplicate clone's protocol handling — follow-up), `--source`
(push an existing local repo).

## Next: spec 055 — #100 `bb repo fork`
