# 036 tui: PR list view

## Goal
`bb dash` opens to a pull-request table for the current repo (default section
"Open PRs"). The first real, navigable view.

## Command surface
Internal view under `tui/views/pr.rs`. Keys: `j/k`/↑↓ move selection, `g/G`
top/bottom, `r` refresh, `Tab` switch section (placeholder until 041), `q` quit.

## Bitbucket endpoint(s)
None new — calls `pr::query::list` (spec 033) via the worker (spec 035).

## Behavior & edge cases
- Columns: `#`, `TITLE`, `BRANCH`, `STATE`, `CI`, `✓` (approvals). Title/branch
  sanitized (`render::sanitize`) and truncated to column width.
- Loading → spinner + "Loading pull requests…". Empty → centered "No pull requests".
- Selection clamps to row count; persists across `r` refresh by PR id when possible.
- Colors via the theme (037/042); state/CI colored (open=green, declined=red, etc.).

## Test cases
- Reducer: selection move up/down clamps at bounds; `g`/`G` jump to ends.
- Rows built from `Vec<PullRequest>` carry id/title/branch/state.
- Empty-state and loading-state render via `TestBackend` without panic.

## Out of scope
Detail pane (037), actions (038), real sections/filtering (041).

## Next: spec 037 — PR detail pane
