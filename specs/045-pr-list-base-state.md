# 045 pr list: `--base` must keep the `--state` filter

Fixes #114.

## Goal / user story
`bb pr list --state MERGED --base main` should list only MERGED PRs targeting
`main`. Today it lists PRs of *every* state, because the command sends both a
`state=` query param **and** a `q=` BBQL param, and Bitbucket ignores `state=`
when `q=` is present — and the `q` only carries the destination clause, so all
states come back. The default (`--state OPEN`) is dropped the same way.

## Command surface
`bb pr list [--state STATE] [--base BRANCH] [-L N]` — no flag changes.

## Bitbucket endpoint(s)
`GET /2.0/repositories/{ws}/{slug}/pullrequests`

- No `--base`: `?state={STATE}&pagelen={n}` (unchanged).
- With `--base`: `?pagelen={n}&q=<encoded>` where the BBQL combines both:
  `state="{STATE}" AND destination.branch.name="{BRANCH}"`. The standalone
  `state=` param is dropped (it would be ignored anyway).

## Behavior & edge cases
- State value comes from `args.state` (clap default `OPEN`; one of
  OPEN/MERGED/DECLINED/SUPERSEDED).
- `q` is percent-encoded via `render::percent_encode`.
- No `--base` path is untouched (still uses `state=`), so existing behavior and
  tests for the common case are preserved.

## Test cases (red first)
- `list_base_combines_state_and_destination_in_query` (replaces
  `list_base_adds_query_filter`): with `--base main` (default state OPEN), the
  URL contains `q=` with the encoded `state="OPEN" AND
  destination.branch.name="main"`, and has **no** standalone `&state=` param.
- `list_base_uses_given_state`: `--state MERGED --base main` → `q` contains
  `state="MERGED"`.

## Out of scope
Author/reviewer filters (`pr status`, #98).

## Next: spec 046 — #94 `bb api` typed fields (`-F`/`--field`)
