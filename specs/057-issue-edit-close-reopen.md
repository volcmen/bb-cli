# 057 issue: `edit` / `close` / `reopen`

Fixes #93.

## Goal / user story
`bb issue` can create/list/view/comment but cannot update an issue or change its
state. Add `edit`, `close`, and `reopen` (parity with `gh issue
edit/close/reopen`), completing the issue command family.

## Command surface
- `bb issue edit <id> [--title T] [--body B|--body-file F] [--kind K]
  [--priority P] [--state S]` — at least one field, else `FlagError`.
- `bb issue close <id>` — set state `resolved`.
- `bb issue reopen <id>` — set state `open`.

States: new, open, resolved, on hold, invalid, duplicate, wontfix, closed.
Exit codes: `AuthError`(4); `FlagError`(1) nothing-to-update / bad input;
tracker-disabled (410/404) → the shared `issue::tracker_disabled` message;
issue-not-found (404) for a present tracker → "issue #N not found".

## Bitbucket endpoint(s)
1. `GET …/issues/{id}` — current issue.
2. `PUT …/issues/{id}` `{ title, content:{raw}, kind?, priority?, state }`,
   merging current values with overrides (so unspecified fields are preserved).

## Behavior & edge cases
- close → state `resolved`; reopen → state `open` (the rest preserved).
- edit merges each provided field, keeps the rest.
- 410 → tracker disabled; 404 → issue not found (checked in that order).

## Test cases (red first)
- `edit_updates_title_via_put` (preserves body/state).
- `edit_state_change`, `edit_no_fields_is_flag_error`.
- `close_sets_resolved`, `reopen_sets_open`.
- `edit_tracker_disabled_410_reports_tracker`.
- `edit_not_authed_is_auth_error`.

## Out of scope
Assignee editing. Bulk operations.

## Next: spec 058 — #102 `bb repo delete` (guarded)
