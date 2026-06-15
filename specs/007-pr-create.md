# 007 bb pr create

## Goal
Open a pull request for the current (or chosen) branch.

## Command surface
`bb pr create [-t TITLE] [-b BODY | -F FILE] [-B BASE] [-H HEAD] [--web] [--close-source-branch] [--reviewer a,b]`
Exit: 0; 1 FlagError (no title non-interactive); 4 AuthError.

## Endpoints
- Default base: `GET /2.0/repositories/{ws}/{slug}` → `mainbranch.name` (else "main").
- Create: `POST .../pullrequests` body `{title, source:{branch:{name:HEAD}}, destination:{branch:{name:BASE}}, description, close_source_branch}` → print `links.html.href`.

## Behavior & edge cases
- repo via `base_repo()`; not authed → AuthError. head = `--head` or current branch.
- body: `--body` | `--body-file` ("-"=stdin) | "".
- title: flag, else prompt (default head), else FlagError when non-interactive.
- repo lookup error propagates (no silent base=main on failure).
- `--reviewer`: noted as Epic 1 (uuid resolution), ignored for now.
- `--web`: open compare URL `…/pull-requests/new?source=HEAD&dest=BASE` (branch names url-encoded); no API call.

## Tests
Happy POST (asserts payload + URL); default base from mainbranch; repo-lookup error propagates; non-interactive missing title → FlagError; not-logged-in → AuthError; `--web` opens browser + prints encoded URL.

## Next: spec 008 — pr list (#17)
