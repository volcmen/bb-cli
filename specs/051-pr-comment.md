# 051 pr: `bb pr comment`

Fixes #96.

## Goal / user story
Add or read comments on a pull request from the CLI (parity with `gh pr
comment`). Today `bb pr` can view a PR but not comment on it.

## Command surface
`bb pr comment [ID] [--body B | --body-file FILE | (editor when interactive)]
[--list]`

- `ID` optional; defaults to the current branch's PR (finder).
- Default action posts a comment; `--list` prints existing comments instead.
- Body precedence (post): `--body`, then `--body-file` (`-` => stdin), then an
  editor when interactive, else `FlagError`.
- Exit codes: `AuthError`(4); `FlagError`(1) for bad id / missing body / PR not
  found.

## Bitbucket endpoint(s)
- Post: `POST /2.0/repositories/{ws}/{slug}/pullrequests/{id}/comments`
  `{content:{raw}}`.
- List: `GET …/pullrequests/{id}/comments` (paginated); skip `deleted` comments.

## Behavior & edge cases
- Post → "✓ Commented on pull request #{id}".
- `--list` → one block per non-deleted comment: `@{author}:` then the raw body;
  empty → "No comments on pull request #{id}.".
- `--list` ignores `--body`/`--body-file`.

## Test cases (red first)
- `comment_posts_content`: `--body hi` → POST body `content.raw == "hi"` +
  confirmation.
- `comment_body_file_reads_file`.
- `comment_list_renders_comments`: GET stub with 2 comments (one deleted) →
  output has both authors? no — deleted skipped; shows the live one.
- `comment_no_body_non_interactive_is_flag_error` (no POST).
- `comment_not_authed_is_auth_error`.

## Out of scope
Inline/file comments, replies (`--reply-to`) — follow-up. Editing/deleting
comments.

## Next: spec 052 — #97 `bb pr review` (request-changes)
