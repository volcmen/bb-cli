# 069 snippet: `bb snippet` CRUD

Fixes #107. Parity with `gh gist` (Bitbucket Snippets are the analog).

## Goal / user story
Create and manage Bitbucket Snippets from the CLI.

## Command surface
- `bb snippet create [FILES…] [--title T] [--private] [--workspace WS]`
  - `POST /2.0/snippets` (personal) or `/2.0/snippets/{ws}` (multipart/form-data).
  - One file part per FILE (the **field name is the filename**, per Bitbucket),
    plus `title` and `is_private` fields. ≥1 file required (`FlagError`).
  - Prints `WS/ID` and the html URL.
- `bb snippet list [--workspace WS] [--limit N]`
  - Paginate `/2.0/snippets[/{ws}]`; table of `WS/ID`, privacy, title.
- `bb snippet view <WS/ID | ID --workspace WS> [--web]`
  - `GET /2.0/snippets/{ws}/{id}`; prints title, privacy, files, URL. `--web`
    opens the html URL in the browser.
- `bb snippet edit <WS/ID> [FILES…] [--title T]`
  - multipart `PUT`; requires at least one of FILES/`--title`.
- `bb snippet delete <WS/ID> [--yes]`
  - confirm (unless `--yes`/non-interactive `--yes`), then `DELETE`.
- `bb snippet clone <WS/ID> [DIR]`
  - `GET` the snippet, then `git clone` its clone URL (honoring `git_protocol`).

## Identity
Snippets are workspace-scoped. The id arg may be `WS/ID` or a bare `ID` plus
`--workspace`. A bare id without `--workspace` is a `FlagError` (no implicit
`/2.0/user` lookup).

## API / seam changes
- `api::models::Snippet` (`id`, `title`, `is_private`, `owner`, `created_on`,
  `links` (html+clone), `files: { name → SnippetFile }`).
- `BitbucketClient::send_multipart<T>(method, path, parts)` building a
  `multipart/form-data` body (the existing `build_request` always sets JSON, so
  multipart needs its own request path). Reuses auth header + base URL +
  error mapping.

## Test cases (red first)
- client: `send_multipart` sets a `multipart/form-data` content type and a body
  containing each part's disposition + value; non-2xx → mapped error.
- create: posts multipart with file/title/is_private; no files → FlagError;
  prints id+url; `--workspace` targets `/2.0/snippets/{ws}`.
- list: renders rows; `--limit` honored.
- view: prints title/files; bad id (bare, no --workspace) → FlagError; `--web`
  opens the browser (RecordingBrowser).
- edit: multipart PUT; nothing to change → FlagError.
- delete: `--yes` deletes without prompt; declined confirm → CancelError.
- clone: GETs then `git clone <url>`.
- not-authed → AuthError (each network subcommand).

## Out of scope
Snippet comments/watchers; per-file deletion semantics beyond replace-on-name.

## Next: spec 070 — (epic #95 follow-ups / TUI epic #79)
