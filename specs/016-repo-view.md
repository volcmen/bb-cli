# 016 bb repo view

## Goal
Show a repository's details.

## Command surface
`bb repo view [WORKSPACE/SLUG] [--web]`. Exit 0; 4 AuthError; 1 not-found.

## Endpoint
`GET /2.0/repositories/{ws}/{slug}` → Repository (name, full_name, is_private, description, mainbranch, links.html, links.clone[]).

## Behavior & edge cases
- Default to `ctx.base_repo()` when no arg; else parse `WORKSPACE/SLUG` (`RepoId::from_str`).
- `--web` opens `links.html.href`. not-found → exit 1.
- Render: full_name, visibility (private/public), description, default branch, web URL.

## Tests
view by ws/slug (renders); default repo; --web; not-found; not-authed → AuthError.

## Next: spec 017 — repo clone (#28)
