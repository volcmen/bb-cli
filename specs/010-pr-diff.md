# 010 bb pr diff

## Goal
Print a pull request's unified diff.

## Command surface
`bb pr diff [ID]`. Exit 0; 4 AuthError; 1 not-found/invalid id.

## Endpoint
`GET /2.0/repositories/{ws}/{slug}/pullrequests/{id}/diff` → `text/plain` (use `client.get_raw`).

## Behavior & edge cases
- Resolve PR via shared finder (id or branch inference).
- Print raw diff to stdout (no added color in MVP; pipe-friendly).

## Tests
diff by id prints body; branch inference; not-found.

## Next: spec 011 — pr merge (#21)
