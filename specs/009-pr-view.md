# 009 bb pr view  (Epic 1 — STUB)

## Goal
Show a single pull request's details; infer from current branch when no id given.

## Command surface (proposed)
`bb pr view [ID] [--web] [--json FIELDS] [--jq EXPR]`. Exit 0; 4 AuthError; 1 not-found.

## Endpoints
- By id: `GET /2.0/repositories/{ws}/{slug}/pullrequests/{id}`.
- By branch (no id): `GET .../pullrequests?q=source.branch.name="CURRENT"&state=OPEN` → first match.

## Behavior & edge cases (to refine)
- `--web` opens `links.html.href`. Not-found → clear error (exit 1).
- Render: title, state, source→dest, author, description; reviewers/approvals (Epic 1).

## Test cases (to write first)
view by id; branch inference; `--web` opens href; not-found; not-authed → AuthError.

## Out of scope
diff/merge/approve (separate Epic 1 specs).

## Next: spec 010 — pr diff
