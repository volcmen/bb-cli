# 038 tui: PR actions + confirm/input modals

## Goal
Act on the selected PR without leaving the dashboard — the gh-dash value
proposition. All actions reuse the spec-033 `pr::actions` layer.

## Command surface
Internal. Keys on a selected PR: `c` checkout branch, `a` approve / un-approve
(toggle), `m` merge, `x` decline, `o` open in browser, `C` comment, `y` copy URL.

## Bitbucket endpoint(s)
Reuses `pr::actions::{approve,unapprove,merge,decline,comment}` + `GitClient`
(checkout = `fetch` then `checkout`) + `Browser::browse` (spec 033 / seam traits).

## Behavior & edge cases
- **Destructive actions (`m` merge, `x` decline) require a confirm modal** (y/N).
- `C` comment opens an input modal (multi-line; `Ctrl-Enter`/`Esc` submit/cancel).
- After a successful mutation → refresh the affected PR + a success toast; on error
  → error toast, no state corruption.
- `c` checkout runs git in the worker; success toast names the branch; failure
  (dirty tree, missing remote) → toast with the git error, dashboard stays open.
- Modals capture all input; `Esc` cancels; background list dimmed.

## Test cases
- Reducer: opening/confirming/cancelling a modal transitions correctly; `Esc` cancels.
- `a` dispatches `Request::Approve(id)`; `FakeTransport` asserts the approve POST.
- Merge requires confirm before any `Request` is dispatched (no accidental merge).
- Comment modal submit dispatches the comment with the typed body.

## Out of scope
Diff viewer (could be a later enhancement / `d` → shell out). Issue actions (039).

## Next: spec 039 — Issues view (list/detail/actions)
