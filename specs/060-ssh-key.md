# 060 ssh-key: `bb ssh-key list/add/delete`

Fixes #109.

## Goal / user story
Manage your account's SSH keys from the CLI (parity with `gh ssh-key`).
(Bitbucket has no GPG-key API, so `gh gpg-key` stays out of scope.)

## Command surface
- `bb ssh-key list`
- `bb ssh-key add <PATH|-> [--title T]` — read the public key from a file or
  stdin.
- `bb ssh-key delete <KEY-UUID>`

Exit codes: `AuthError`(4); `FlagError`(1) bad input / IO.

## Bitbucket endpoint(s)
Resolve the account uuid via `GET /2.0/user`, then:
- list: `GET /2.0/users/{uuid}/ssh-keys`
- add: `POST /2.0/users/{uuid}/ssh-keys` `{ key, label? }`
- delete: `DELETE /2.0/users/{uuid}/ssh-keys/{key-uuid}`

The uuid (`{...}`) is percent-encoded into the path.

## Behavior & edge cases
- `list` prints `{label}\t{key}` per key; empty → "No SSH keys.".
- `add` trims the key text; `--title` → `label`. Prints "✓ Added SSH key".
- `delete` prints "✓ Deleted SSH key {uuid}".

## Test cases (red first)
- `list_prints_keys` / `list_empty`.
- `add_posts_key_from_file` (label + key in body).
- `delete_sends_delete`.
- `not_authed_is_auth_error`.

## Out of scope
GPG keys (no API). Key validation beyond non-empty.

## Next: spec 061 — #105 `bb pipeline run/stop/logs`
