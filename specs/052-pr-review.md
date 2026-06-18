# 052 pr: `bb pr review` (approve / request-changes / comment)

Fixes #97.

## Goal / user story
`bb pr` can approve (and un-approve) but has no `gh pr review` umbrella with
**request-changes** or a review **comment**. Add `bb pr review` with the three
review actions.

## Command surface
`bb pr review [ID] (--approve | --request-changes | --comment) [--body B |
--body-file FILE]`

- Exactly one of `--approve` / `--request-changes` / `--comment` required, else
  `FlagError`.
- `--comment` requires a body (`--body`/`--body-file`, or editor when
  interactive), else `FlagError`.
- `ID` optional; defaults to the current branch's PR (finder).
- `--approve` is equivalent to `bb pr approve` (kept as a shortcut too).

## Bitbucket endpoint(s)
- approve: `POST …/pullrequests/{id}/approve`
- request-changes: `POST …/pullrequests/{id}/request-changes`
- comment: `POST …/pullrequests/{id}/comments` `{content:{raw}}`

## Behavior & edge cases
- approve → "✓ Approved pull request #{id}".
- request-changes → "✓ Requested changes on pull request #{id}".
- comment → "✓ Commented on pull request #{id}".
- `--body` is ignored by approve/request-changes (only used by comment).

## Test cases (red first)
- `review_approve_posts`, `review_request_changes_posts`,
  `review_comment_posts` (body in POST).
- `review_no_mode_is_flag_error`, `review_multiple_modes_is_flag_error`.
- `review_comment_without_body_is_flag_error` (non-interactive).
- `review_not_authed_is_auth_error`.

## Out of scope
Withdrawing a review (request-changes DELETE) — `pr approve --undo` already
covers approval withdrawal. Inline review comments.

## Next: spec 053 — #98 `bb pr status`
