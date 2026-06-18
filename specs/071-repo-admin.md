# 071 repo admin: webhooks / deploy-keys / branch-restrictions / default-reviewers

Fixes #113. Bitbucket-native repo admin (beyond `gh`; closest is `gh ruleset`).

## Scope blocker (same as #52)
The embedded OAuth consumer lacks `repository:admin`, so these will return
"credentials lack required privilege scopes" live until a broader consumer ships
(#52). They are built + fully unit-tested now (the established pattern for the
repo create/edit/delete admin commands), and work live once the scope is granted.

## Command surface (all under `bb repo`, resolving the repo via `ctx.base_repo()`)
- `bb repo webhook list|create|delete`
  - list: `GET .../hooks`. create: `POST .../hooks` `{url, description, active,
    events}` (`--url` required, `--event` repeatable default `repo:push`,
    `--description`). delete `DELETE .../hooks/{uuid}`.
- `bb repo deploy-key list|add|delete`
  - list: `GET .../deploy-keys`. add: `POST .../deploy-keys` `{key, label}`
    (`--key` or `--key-file`, `--title`). delete: `DELETE .../deploy-keys/{id}`.
- `bb repo branch-restriction list|create|delete`
  - list: `GET .../branch-restrictions`. create: `POST` `{kind,
    branch_match_kind:"glob", pattern}` (`--kind`, `--pattern`). delete by `{id}`.
- `bb repo default-reviewer list|add|remove`
  - list: `GET .../default-reviewers` (User[]). add/remove:
    `PUT|DELETE .../default-reviewers/{user}`.

All: `AuthError` when unauthenticated; `FlagError` for missing required flags.

## Models
`Webhook {uuid,url,description,active,events}`, `DeployKey {id,label,key}`,
`BranchRestriction {id,kind,pattern,branch_match_kind}`; default-reviewers reuse
`User`.

## Test cases (red first), per area
- list renders rows; create/add POSTs the expected body / PUT path; delete hits
  the id/uuid path; missing-required-flag → FlagError; unauthenticated → AuthError.

## Out of scope
Update/patch of existing hooks/restrictions; webhook secret management;
branch-restriction user/group exemptions.

## Next: epic #95 complete; remaining work is the TUI epic #79 and release #57.
