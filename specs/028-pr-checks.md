# 028 bb pr checks

## Goal
Show the build/CI statuses on a PR's head commit (gh `pr checks` analog).

## Command surface
`bb pr checks [ID] [--json …]`. Exit 0 (all pass / none); **1 if any check FAILED**; 4 AuthError.

## Endpoint
- Resolve PR via shared finder (id or branch). Head commit = `pr.source.commit_hash()`.
- `GET /2.0/repositories/{ws}/{slug}/commit/{sha}/statuses` → paginated `CommitStatus` values (`key`, `state` ∈ SUCCESSFUL/FAILED/INPROGRESS/STOPPED, `name`, `url`).

## Behavior & edge cases
- TTY table (state, name/key, url) / TSV. Empty → "no checks reported for PR #id". `--json`.
- **Exit 1 if any status is FAILED** (CI-friendly); else exit 0. not-authed → AuthError.

## Tests
checks render; any-failed → non-zero exit; empty message; --json; not-authed → AuthError.

## Next: Epic 6 — browse + api passthrough (specs 029+)
