# 056 repo: `bb repo edit` / `bb repo rename`

Fixes #101.

## Goal / user story
Update a repository's settings (description, visibility, project) and rename it,
from the CLI (parity with `gh repo edit` / `gh repo rename`).

## Command surface
- `bb repo edit [--description D] [--visibility public|private] [--project KEY]`
  — at least one field required, else `FlagError`.
- `bb repo rename <NEW-NAME>`.

Both operate on the current repo (`base_repo`, or `-R`). Exit codes:
`AuthError`(4); `FlagError`(1) nothing-to-update.

## Bitbucket endpoint(s)
`PUT /2.0/repositories/{ws}/{slug}`:
- edit → `{ description?, is_private?, project:{key}? }`
- rename → `{ name }`

Needs `repository:admin` at runtime (see #52).

## Behavior & edge cases
- `--visibility private` → `is_private:true`; `public` → `false`.
- Body omits unset fields.
- edit prints `✓ Updated {ws}/{slug}`; rename prints `✓ Renamed {ws}/{slug} → {name}`.

## Test cases (red first)
- `edit_updates_description`, `edit_visibility_private`, `edit_visibility_public`.
- `edit_no_fields_is_flag_error` (no HTTP).
- `rename_puts_name`.
- `edit_not_authed_is_auth_error`.

## Out of scope
`--default-branch`, enabling issues/wiki (admin-scope dependent). `repo delete`
(#102), `repo set-default`/`sync` (#103).

## Next: spec 057 — #102 `bb repo delete` (guarded)
