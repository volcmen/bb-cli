# 025 bb issue comment

## Goal
Add a comment to an issue.

## Command surface
`bb issue comment ID [-b BODY | -F FILE]`. Exit 0; 1 FlagError (empty body non-interactive); 4 AuthError.

## Endpoint
`POST /2.0/repositories/{ws}/{slug}/issues/{id}/comments` body `{content:{raw: BODY}}` → comment; print confirmation.

## Behavior & edge cases
- Body from `--body`/`--body-file`(`-`=stdin), else prompt (editor) when interactive, else FlagError. not-found / not-authed handled.

## Tests
comment (assert content body); empty non-interactive → FlagError; not-authed → AuthError.

## Next: Epic 5 — pipelines & checks (specs 026+)
