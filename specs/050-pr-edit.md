# 050 pr: `bb pr edit`

Fixes #91.

## Goal / user story
Update an existing pull request from the CLI — title, description, base branch —
without opening the browser (the question "can it change a PR's description?").
Parity with the core of `gh pr edit`.

## Command surface
`bb pr edit [ID] [--title T] [--body B | --body-file FILE] [--base BRANCH]`

- `ID` optional; defaults to the PR for the current branch (via `finder`, like
  `merge`/`close`).
- At least one of `--title`/`--body`/`--body-file`/`--base` required, else
  `FlagError` ("nothing to update").
- `--body-file -` reads stdin.
- Exit codes: `AuthError`(4) unauthenticated; `FlagError`(1) nothing-to-update /
  bad id / PR not found.

## Bitbucket endpoint(s)
1. `GET /2.0/repositories/{ws}/{slug}/pullrequests/{id}` — current PR.
2. `PUT  …/pullrequests/{id}` with `{ title, description, destination:{ branch:{
   name }}}`, merging current values with the requested overrides (so omitted
   fields are preserved, not cleared).

## Behavior & edge cases
- Title override → `--title`, else keep current.
- Description override → `--body`/`--body-file`, else keep current
  (`pr.description`).
- Base override → `--base`, else keep current `destination.branch.name`.
- Prints the PR URL on success.

## Test cases (red first)
- `edit_updates_title_via_put`: stub GET + PUT; `--title New` → PUT body has the
  new title and preserved description/destination.
- `edit_body_file_dash_reads_stdin`.
- `edit_base_changes_destination`.
- `edit_no_fields_is_flag_error` (no HTTP calls).
- `edit_not_authed_is_auth_error`.
- `put` client method unit (via the command path).

## Out of scope
Editing reviewers (`--add-reviewer`/`--remove-reviewer`) — follow-up, needs
read-merge of the reviewer list. Draft/state (Bitbucket has no draft).

## Next: spec 051 — #96 `bb pr comment`
