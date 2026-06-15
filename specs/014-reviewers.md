# 014 Reviewer uuid resolution (--reviewer)  (STUB)

## Goal
Resolve `--reviewer` usernames/emails to Bitbucket uuids and attach them on
`pr create` (and `pr edit` later).

## Endpoint(s)
- `GET /2.0/workspaces/{ws}/members?q=...` (or user lookup) → match to `uuid`.
- Feed `reviewers: [{uuid}]` into the create payload.

## Behavior & edge cases
- Unknown user → clear error listing the unresolved name.
- Cache/batch lookups when multiple reviewers.

## Tests (to write first)
resolves a known member; unknown → error; integrates into pr create payload.

## Next: spec 015 — pr checkout (#25)
