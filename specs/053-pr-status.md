# 053 pr: `bb pr status`

Fixes #98.

## Goal / user story
`bb pr status` shows the pull requests in the current repo that are relevant to
me — ones I authored and ones awaiting my review (parity with the core of `gh pr
status`).

## Command surface
`bb pr status` (no positional). Two sections:
- **Created by you** — open PRs you authored.
- **Requesting your review** — open PRs where you are a reviewer.

Exit codes: `AuthError`(4) unauthenticated; propagates API errors.

## Bitbucket endpoint(s)
1. `GET /2.0/user` → your `uuid`.
2. Created: `GET …/pullrequests?q=state="OPEN" AND author.uuid="{uuid}"`.
3. Review: `GET …/pullrequests?q=state="OPEN" AND reviewers.uuid="{uuid}"`.

State is folded into the BBQL `q` (Bitbucket ignores `state=` when `q` is
present — see #114).

## Behavior & edge cases
- Each section prints `#{id}  {title}  ({src} → {dst})` per PR; empty → `(none)`.
- The created query is issued before the review query.
- Title sanitized via `render::sanitize`.

## Test cases (red first)
- `status_lists_authored_and_review_sections`: stub `/user` + two
  `/pullrequests` pages (author then review) → both sections render with their
  PRs.
- `status_empty_sections_show_none`.
- `status_not_authed_is_auth_error`.

## Out of scope
The "current branch" section, `--json`, custom limits.

## Next: spec 054 — #99 `bb repo create`
