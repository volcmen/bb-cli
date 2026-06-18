# 065 pr: `bb pr edit --add-reviewer / --remove-reviewer`

Fixes #117. Follow-up split from #91/#050 (reviewer editing was deferred there).

## Goal / user story
Add or remove reviewers on an existing PR without re-creating it. Parity with
`gh pr edit --add-reviewer/--remove-reviewer`. Bitbucket's `PUT` does not expose a
delta — the whole reviewer set must be sent — so the command read-merges the
current reviewers with the requested adds/removes.

## Command surface
`bb pr edit [ID] [--add-reviewer <user>]… [--remove-reviewer <user>]…`

- Both flags repeatable (also comma-separated, matching `pr create --reviewer`).
- Compose with the existing `--title`/`--body`/`--base`.
- These two flags now also satisfy the "at least one change" guard.
- Exit codes unchanged: `AuthError`(4); `FlagError`(1) nothing-to-update / bad id /
  PR not found / unresolvable add-reviewer.

## Bitbucket endpoint(s)
1. `GET …/pullrequests/{id}` — current PR (already fetched; carries `reviewers[]`).
2. `GET /2.0/workspaces/{ws}/members` — only when `--add-reviewer` is non-empty,
   to resolve names→UUID (reuse `pr create`'s `resolve_reviewers`/`member_matches`).
3. `PUT …/pullrequests/{id}` with `reviewers: [{uuid}…]` included **only** when a
   reviewer edit was requested (so plain title/body edits keep preserving reviewers
   by omission).

## Behavior & edge cases
- Start from the current PR's reviewer UUIDs (in order).
- `--remove-reviewer S`: drop any current reviewer matching `S` on uuid/account_id/
  username/nickname/display_name (case-insensitive) — works even for a member who
  left the workspace (matched against the PR's own reviewer objects, no member fetch).
- `--add-reviewer S`: resolve `S` against workspace members → uuid; append if not
  already present (dedup by uuid). Unresolvable → `FlagError` naming them.
- Remove is applied before add; adding someone you also removed re-adds them.
- No reviewer flags → `reviewers` omitted from PUT (current behavior preserved).

## Test cases (red first)
- `edit_add_reviewer_merges_into_current`: current `[alice]`; `--add-reviewer bob`
  → members fetch + PUT body reviewers uuids = {alice, bob}.
- `edit_remove_reviewer_drops_from_current`: current `[alice, bob]`;
  `--remove-reviewer alice` → PUT reviewers = {bob}; **no** members fetch.
- `edit_add_and_remove_compose`: current `[alice]`; add bob, remove alice → {bob}.
- `edit_add_unknown_reviewer_is_flag_error` (after members fetch; no PUT).
- `edit_reviewer_flag_satisfies_change_guard` (no title/body still runs).
- Existing edit tests still pass (reviewers omitted when no reviewer flag).

## Out of scope
Reordering reviewers; default-reviewer auto-population; participants/approvals.

## Next: spec 066 — #103 `bb repo set-default` / `bb repo sync`
