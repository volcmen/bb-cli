# 061 search: `bb search repos / code / prs`

Fixes #108.

## Goal / user story
Search Bitbucket from the CLI (parity with the core of `gh search`): repositories
by name, source code, and pull requests.

## Command surface
- `bb search repos <QUERY> [--workspace WS] [-L N]`
- `bb search code <QUERY> [--workspace WS] [-L N]`
- `bb search prs <QUERY> [-L N]` (current repo)

`--workspace` defaults to the current repo's workspace (`base_repo`). Exit codes:
`AuthError`(4); propagates API errors.

## Bitbucket endpoint(s)
- repos: `GET /2.0/repositories/{ws}?q=name~"QUERY"&pagelen=N`
- code: `GET /2.0/workspaces/{ws}/search/code?search_query=QUERY&pagelen=N`
- prs: `GET /2.0/repositories/{ws}/{slug}/pullrequests?q=title~"QUERY"&pagelen=N`

BBQL `q` and the code `search_query` are percent-encoded.

## Behavior & edge cases
- repos → `{full_name}\t{description}` per hit; empty → "No repositories match.".
- code → the file path per hit (`values[].file.path`); empty → "No code matches.".
- prs → `#{id}\t{title}` per hit; empty → "No pull requests match.".
- `-L` clamps pagelen to [1,50].

## Test cases (red first)
- `repos_search_builds_query_and_lists`.
- `code_search_lists_paths`.
- `prs_search_lists`.
- `search_not_authed_is_auth_error`.

## Out of scope
Commit/issue search; ranking/highlighting; cross-workspace search.

## Next: spec 062 — #106 `bb variable` (Pipelines variables)
