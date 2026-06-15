# 013 bb pr approve

## Goal
Approve a pull request, or remove your approval.

## Command surface
`bb pr approve [ID] [--undo]`. Exit 0; 4 AuthError.

## Endpoints
- Approve: `POST .../pullrequests/{id}/approve` → participant `{approved:true}`.
- Unapprove: `DELETE .../pullrequests/{id}/approve` (`--undo`).

## Behavior & edge cases
- Resolve via finder. Print "Approved PR #id" / "Removed approval".
- VERIFY: `request-changes` availability on Cloud (separate flag later if supported).

## Tests
approve (POST); undo (DELETE via send_empty); not-authed → AuthError.

## Next: spec 014 — reviewer resolution (#24)
