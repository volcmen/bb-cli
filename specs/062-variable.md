# 062 variable: `bb variable list/set/delete` (Pipelines variables)

Fixes #106.

## Goal / user story
Manage Bitbucket Pipelines variables from the CLI (gh `variable` + `secret`
parity — Bitbucket has one "variables" resource with a `secured` flag, so a
secret is just `--secured`).

## Command surface
- `bb variable list [--workspace WS]`
- `bb variable set <KEY> --value <V> [--secured] [--workspace WS]`
- `bb variable delete <KEY> [--workspace WS]`

Scope: repo by default; `--workspace WS` targets workspace variables. Exit
codes: `AuthError`(4); `FlagError`(1) when deleting a missing key.

## Bitbucket endpoint(s)
- repo: `…/repositories/{ws}/{slug}/pipelines_config/variables/`
- workspace: `…/workspaces/{ws}/pipelines-config/variables/`  (note hyphen)
- list `GET`; create `POST {key,value,secured}`; update `PUT {uuid}`; delete
  `DELETE {uuid}`. `set` upserts: list → if the key exists, PUT its uuid, else
  POST. `delete` resolves the key→uuid via list.

## Behavior & edge cases
- `list` prints `{key}\t{value-or-(secured)}`; empty → "No variables.".
- `set` prints "✓ Set variable {key}"; `--secured` → `secured:true` (value
  write-only, never echoed).
- `delete` of a missing key → `FlagError`.

## Test cases (red first)
- `list_prints_variables` (incl. a secured one shown as `(secured)`).
- `set_creates_when_absent` (POST body key/value/secured).
- `set_updates_when_present` (PUT to `.../{uuid}`).
- `delete_resolves_key_and_deletes`; `delete_missing_is_flag_error`.
- `workspace_scope_uses_workspace_endpoint`.
- `not_authed_is_auth_error`.

## Out of scope
Deployment-environment variables (a third scope) — follow-up.

## Next: spec 063 — #105 `bb pipeline run/stop/logs`
