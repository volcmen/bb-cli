# 040 tui: Pipelines section + live auto-refresh

## Goal
A Pipelines section showing recent CI runs with **live status** — the standout TUI
win over the one-shot CLI: in-progress builds update in place on the tick timer.

## Command surface
Internal `tui/views/pipeline.rs` + `pipeline_detail.rs`. Keys: `j/k/g/G` nav,
`Enter` step detail, `r` refresh, `o` open in browser.

## Bitbucket endpoint(s)
Reuses `pipeline::query::{list,get,steps}` (spec 033) via the worker (spec 035).

## Behavior & edge cases
- Columns: `#` (build number), `STATE`/result (colored: pass=green, fail=red,
  in-progress=yellow spinner), `REF`, `CREATED`.
- **Auto-refresh:** while any visible pipeline is `IN_PROGRESS`/`PENDING`, the tick
  timer schedules a re-fetch every N seconds; polling stops once all are terminal.
  Configurable interval (default ~5s), bounded so it never hammers the API.
- Detail: per-step name + state.

## Test cases
- Reducer: a running pipeline schedules a refetch on tick; a fully-terminal list
  stops scheduling (no infinite polling).
- Rows colored by `state_name`/`result_name`; step detail renders.
- `o` opens the pipeline `html_url`.

## Out of scope
Triggering/stopping pipelines (write ops, later). Log streaming.

## Next: spec 041 — sections + fuzzy filter
