# 023 bb issue view

## Goal
Show a single issue.

## Command surface
`bb issue view ID [--web] [--json …]`. Exit 0; 4 AuthError; 1 not-found.

## Endpoint
`GET /2.0/repositories/{ws}/{slug}/issues/{id}` → Issue (title, state, kind, priority, content.raw, reporter, links.html).

## Behavior & edge cases
- `--web` opens `links.html.href`; not-found → exit 1. Render: #id title, state/kind/priority, reporter, body (content.raw, placeholder if empty).

## Tests
view by id; --web; not-found; --json; not-authed → AuthError.

## Next: spec 024 — issue create (#37)
