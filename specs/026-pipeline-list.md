# 026 bb pipeline list

## Goal
List recent CI pipelines for the repo.

## Command surface
`bb pipeline list [-L LIMIT] [--json …]`. Exit 0; 4 AuthError.

## Endpoint
`GET /2.0/repositories/{ws}/{slug}/pipelines/?sort=-created_on&pagelen=min(LIMIT,50)`; paginated → `Pipeline` values.

## Behavior & edge cases
- TTY table (build#, state, result, ref) / TSV piped. Empty → message. `--json`/`--jq`.
- `state_name()` / `result_name()` helpers on `Pipeline`. not-authed → AuthError.

## Tests
list renders + empty + --json + not-authed → AuthError.

## Next: spec 027 — pipeline view (#41)
