# 022 bb issue list

## Goal
List issues in a repo's tracker.

## Command surface
`bb issue list [--state S] [-L LIMIT] [--json …] [--jq …]`. Exit 0; 4 AuthError.

## Endpoint
`GET /2.0/repositories/{ws}/{slug}/issues?sort=-updated_on&pagelen=min(LIMIT,50)` (+ `q=state="S"`); paginated.

## Behavior & edge cases
- Gate: if the repo's tracker is disabled, surface a clear message (the API returns 404 on `/issues` for repos without the tracker — map to "issue tracker is not enabled for {ws}/{slug}").
- TTY table (id, title, state, kind) / TSV piped. Empty → message. `--json`/`--jq` via shared `output`.

## Tests
list renders; empty; tracker-disabled (404) message; --json; not-authed → AuthError.

## Next: spec 023 — issue view (#36)
