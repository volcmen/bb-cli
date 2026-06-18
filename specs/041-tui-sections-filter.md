# 041 tui: configurable sections + fuzzy filter

## Goal
gh-dash-style **sections** (named saved filters) per tab, a section tab bar, and an
in-list **fuzzy filter** (`/`). Lets users frame the dashboard around their workflow
("Needs my review", "My open PRs", "All open").

## Command surface
Internal. Keys: `Tab`/`Shift-Tab` (or `h`/`l`) cycle sections within the active
domain; `/` opens a filter input that narrows the current list live; `Esc` clears
the filter. Section definitions come from config (spec 042) with built-in defaults.

## Bitbucket endpoint(s)
None new — each section is a `PrFilter`/`IssueFilter` (spec 033) the worker runs.

## Behavior & edge cases
- Built-in default sections shipped so zero-config has useful tabs:
  PRs → "Open", "Needs my review", "Mine"; Issues → "Open", "Mine".
  ("Mine"/"review" resolve via the authenticated user from `account`.)
- `/` filter is client-side fuzzy over the loaded rows (title/branch/author); it
  does **not** refetch. Switching sections refetches.
- Empty filter result → "No matches" without losing the underlying list.
- Section state (selected section per tab) persists for the session.

## Test cases
- Reducer: section switch swaps the active filter and triggers a fetch.
- Fuzzy filter narrows rows by subsequence match; clearing restores them.
- "Needs my review"/"Mine" build the expected query from the current user.

## Staged delivery
This issue ships the **fuzzy filter** (`/`) — the self-contained interactive half,
client-side over the loaded rows. **Sections** (named saved filters + the section
tab bar, incl. "Mine"/"Needs my review", which need the authenticated user's
identity) land with **#89/042**, since the spec defines sections *in config* — so
they belong with the config schema rather than as throwaway built-ins here.

## Out of scope
Persisting per-user section *selection* to disk (config defines them; 042).

## Next: spec 042 — config schema + theming + help overlay
