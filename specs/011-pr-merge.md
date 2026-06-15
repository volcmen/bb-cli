# 011 bb pr merge

## Goal
Merge a pull request.

## Command surface
`bb pr merge [ID] [--strategy merge_commit|squash|fast_forward] [--close-source-branch] [-m MSG]`. Exit 0; 4 AuthError; 1 invalid/conflict.

## Endpoint
`POST .../pullrequests/{id}/merge` body `{merge_strategy, message?, close_source_branch}` → merged PR.

## Behavior & edge cases
- Resolve via finder. Surface 4xx (conflict / not mergeable) with the API message.
- Print confirmation (state + URL).
- VERIFY: async merge (`?async`) — handle 202/task polling later if observed.

## Tests
merge with strategy (assert body); conflict 4xx surfaced; not-authed → AuthError.

## Next: spec 012 — pr close/decline (#22)
