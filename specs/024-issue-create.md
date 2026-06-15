# 024 bb issue create

## Goal
Open a new issue.

## Command surface
`bb issue create [-t TITLE] [-b BODY | -F FILE] [--kind K] [--priority P]`. Exit 0; 1 FlagError (no title non-interactive); 4 AuthError.

## Endpoint
`POST /2.0/repositories/{ws}/{slug}/issues` body `{title, content:{raw: BODY}, kind?, priority?}` → Issue; print `links.html.href`.

## Behavior & edge cases
- Title from flag, else prompt (interactive), else FlagError. Body from `--body`/`--body-file`(`-`=stdin)/empty.
- kind/priority optional (validated by clap). not-authed → AuthError.

## Tests
create (assert payload title/content/kind) + printed URL; non-interactive missing title → FlagError; not-authed → AuthError.

## Next: spec 025 — issue comment (#38)
