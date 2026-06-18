# 058 repo: `bb repo delete` (guarded)

Fixes #102.

## Goal / user story
Delete a repository from the CLI — but safely. Destructive, so it requires an
explicit confirmation (type the full `ws/slug`) or `--yes` for scripts.

## Command surface
`bb repo delete <WORKSPACE/SLUG> [--yes]`

- Interactive (no `--yes`): prompt "Type {ws}/{slug} to confirm deletion:"; a
  mismatch aborts with `CancelError` (exit 2).
- Non-interactive without `--yes`: `FlagError` ("refusing to delete without
  confirmation; pass --yes").
- `--yes`: skip the prompt.
- `AuthError`(4) unauthenticated.

## Bitbucket endpoint(s)
`DELETE /2.0/repositories/{ws}/{slug}` (needs `repository:delete`/admin; see #52).

## Behavior & edge cases
- Confirmation compares the trimmed input to `{ws}/{slug}` exactly.
- On success prints `✓ Deleted {ws}/{slug}`.
- No HTTP call is made when confirmation fails or is unavailable.

## Test cases (red first)
- `delete_with_yes_deletes` (no prompt; DELETE issued).
- `delete_confirmed_by_typing_name` (interactive, input matches → DELETE).
- `delete_wrong_confirmation_is_cancel` (input mismatch → `CancelError`, no HTTP).
- `delete_non_interactive_without_yes_is_flag_error` (no HTTP).
- `delete_not_authed_is_auth_error`.

## Out of scope
Recursively deleting forks; trashing/restore.

## Next: spec 059 — #103 `bb repo set-default` / `sync`
