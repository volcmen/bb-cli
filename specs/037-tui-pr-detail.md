# 037 tui: PR detail pane

## Goal
`Enter` on a PR row opens a detail view: title, state, author, source→destination
branches, description, reviewers + approval state, and CI checks.

## Command surface
Internal `tui/views/pr_detail.rs`. Keys: `j/k` scroll body, `Esc`/`q` back to list,
`o` open in browser (carried from 038 but the binding lives here).

## Bitbucket endpoint(s)
Reuses `pr::query::get` + `pr::query::checks` (spec 033) via the worker.

## Behavior & edge cases
- Description rendered as text wrapped to pane width. **Markdown decision:** ship a
  minimal renderer first (paragraphs + bullet wrapping); leave a seam for
  `tui-markdown`/`termimad` as a later enhancement (note it, don't block on it).
- Reviewers show ✔ approved / ⧗ pending; CI checks list key + state (colored).
- Long bodies scroll; pane never overflows the frame.
- Detail can render while still loading checks (partial render + spinner on the CI row).

## Test cases
- Reducer: `Enter` opens detail for the selected id; `Esc` returns to list preserving
  selection.
- Detail view renders title/branches/approvals via `TestBackend`.
- Body scroll offset clamps to content length.

## Out of scope
Mutating actions (038). Full markdown styling (later enhancement).

## Next: spec 038 — PR actions + modals
