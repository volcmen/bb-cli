# 008 bb pr list

## Goal
List pull requests for the current (or chosen) repo.

## Command surface
`bb pr list [--state OPEN|MERGED|DECLINED|SUPERSEDED] [-L LIMIT] [--base BRANCH]`. Exit 0; 4 AuthError.

## Endpoint
`GET /2.0/repositories/{ws}/{slug}/pullrequests?state=STATE&pagelen=min(LIMIT,50)` (+ url-encoded `q=destination.branch.name="BASE"`); paginate to LIMIT.

## Behavior & edge cases
- Not authed → AuthError. Empty → "No pull requests match your search in {ws}/{slug}.".
- TTY → aligned, colored table with header (ID/TITLE/BRANCH/STATE). Non-TTY → tab-separated, no header/color. Control chars in title/branch sanitized to spaces.

## Tests
Results → table + TSV; empty message; `--base` adds query filter; pagelen clamped to limit; not-logged-in → AuthError; TSV sanitizes tabs.

## Next: Epic 1 — spec 009 pr view (#2 / new issues)
