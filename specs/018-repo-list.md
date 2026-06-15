# 018 bb repo list

## Goal
List repositories in a workspace.

## Command surface
`bb repo list [WORKSPACE] [-L LIMIT]`. Exit 0; 4 AuthError.

## Endpoint
`GET /2.0/repositories/{workspace}?pagelen=min(LIMIT,100)&sort=-updated_on` → paginated Repository values.

## Behavior & edge cases
- Default workspace = current repo's workspace (`ctx.base_repo()`).
- TTY table (full_name/slug, visibility, updated) / TSV when piped. Empty → message.

## Tests
list renders table + TSV; default workspace; empty; not-authed → AuthError.

## Next: Epic 3 — output formats (--json/--jq), specs 019+
