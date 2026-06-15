# 012 bb pr close (decline)

## Goal
Decline (close) a pull request.

## Command surface
`bb pr close [ID] [-m MSG]`. Exit 0; 4 AuthError; 1 invalid.

## Endpoint
`POST .../pullrequests/{id}/decline` body `{message?}` → PR with `state: DECLINED`.

## Behavior & edge cases
- Resolve via finder. Idempotent-friendly message if already declined.
- Print confirmation.

## Tests
decline by id (assert state); optional message; not-authed → AuthError.

## Next: spec 013 — pr approve/review (#23)
