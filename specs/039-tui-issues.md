# 039 tui: Issues section (list / detail / actions)

## Goal
An Issues section mirroring the PR experience: list, detail, and core actions.

## Command surface
Internal `tui/views/issue.rs` + `issue_detail.rs`. Reachable via the section
tab bar (`Tab`/`h`/`l`). Keys mirror PRs: `j/k/g/G` nav, `Enter` detail, `r`
refresh, `o` browser, `C` comment, `n` new issue (input modal: title + kind/priority).

## Bitbucket endpoint(s)
Reuses `issue::query::{list,get}` + `issue::actions::{create,comment}` (spec 033)
via the worker (spec 035).

## Behavior & edge cases
- Columns: `#`, `TITLE`, `KIND`, `PRIORITY`, `STATE`. Repos with issues disabled →
  "Issue tracker not enabled for {repo}" screen (mirror `area:issue` command behavior).
- Detail: title/state/kind/priority/reporter/body (same minimal markdown as 037).
- `n` create posts a new issue then refreshes the list and selects it.

## Test cases
- Reducer: section switch loads issues; selection + detail open/close.
- `issue::actions::create` dispatched from the modal hits the create endpoint
  (`FakeTransport`).
- Issues-disabled repo renders the disabled screen, no crash.

## Out of scope
Issue state transitions beyond create/comment (later). Pipelines (040).

## Next: spec 040 — Pipelines section + live auto-refresh
