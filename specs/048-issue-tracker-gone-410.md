# 048 issue: friendly message when the tracker is disabled (HTTP 410)

Fixes #77.

## Goal / user story
`bb issue list` against a repo whose Bitbucket issue tracker is turned off prints
a raw `HTTP 410 ... Gone`. The commands already special-case a **404** as
"tracker not enabled", but Bitbucket actually returns **410 Gone** for the
`/issues` endpoints when the feature is disabled, so the friendly path never
fires.

## Command surface
No flag changes. Affects `issue list`, `issue view`, `issue create`,
`issue comment`.

## Bitbucket endpoint(s)
`/2.0/repositories/{ws}/{slug}/issues[...]` returns **410 Gone** (and sometimes
404) when the tracker is disabled.

## Behavior & edge cases
- New `ApiError::is_gone()` → `status() == Some(410)`.
- `list`/`create`: a 404 **or** 410 → `FlagError` "issue tracker is not enabled
  for {ws}/{slug}" + a hint line.
- `view`/`comment`: 410 → tracker-not-enabled; 404 stays "issue #N not found"
  (a missing issue, not a disabled tracker). 410 is checked first.
- Shared `issue::tracker_disabled(repo)` builds the message so all four agree.
- Exit code 1 (`FlagError`), unchanged.

## Test cases (red first)
- `is_gone` unit test on `ApiError`.
- `list_410_reports_tracker_disabled` (list.rs) — 410 stub → message.
- `view_410_reports_tracker_disabled` (view.rs) — 410 → tracker message, not
  "issue not found".
- `create_410_reports_tracker_disabled`, `comment_410_reports_tracker_disabled`.

## Out of scope
Enabling the tracker (needs `repository:admin`; see #52). `issue edit/close`
(#93).

## Next: spec 049 — #92 `repo clone` over SSH / token (OAuth-safe)
