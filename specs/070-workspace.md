# 070 workspace: `bb workspace list/members/projects`

Fixes #110. Closest parity with `gh org` (Bitbucket Workspaces).

## Goal / user story
Inspect the workspaces you belong to, plus a workspace's members and projects.

## Deprecation note (CHANGE-2770)
`GET /2.0/workspaces` (list-all) and `?role=` filtering are deprecated. This
command uses the documented replacements:
- list → `GET /2.0/user/permissions/workspaces` (workspaces the caller is in).
- members → `GET /2.0/workspaces/{ws}/members` (not deprecated).
- projects → `GET /2.0/workspaces/{ws}/projects` (not deprecated).

Live behavior depends on token scopes (same caveat as the admin commands /
#52); the command is fully unit-tested against the documented response shapes.

## Command surface
- `bb workspace list [--limit N]` → rows `slug \t permission \t name`.
- `bb workspace members <WS> [--limit N]` → rows of member `username` / display name.
- `bb workspace projects <WS> [--limit N]` → rows `key \t public|private \t name`.
- `AuthError` when unauthenticated.

## Models
- `Workspace { slug, name, uuid, is_private }`.
- `WorkspaceMembership { permission, workspace }` (for the list endpoint).
- `Project { key, name, is_private, description, links }`.
- members reuse the existing `Membership { user }`.

## Test cases (red first)
- `list_renders_workspaces` (values[].workspace.slug/name + permission).
- `members_render_users`.
- `projects_render_rows` (key + privacy + name).
- `--limit` honored (paginate cap).
- not-authed → AuthError (each).

## Out of scope
Creating/editing workspaces or projects; membership management.

## Next: spec 071 — (#113 repo admin, post-mvp) / TUI epic #79.
