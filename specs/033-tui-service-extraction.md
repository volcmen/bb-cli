# 033 refactor: shared service/actions layer (TUI prerequisite)

## Goal
Both the CLI commands and the forthcoming TUI must fetch and mutate the same way.
Today each command `run()` interleaves *fetch* (build client → call API) with
*render-to-stdout*. Extract the fetch/mutation half into a reusable, render-free
**service layer** that both callers share. **No behavior change.**

## Command surface
None. Internal API only. New modules under `crate::service` (or per-domain
`commands/pr/query.rs` + `commands/pr/actions.rs`, etc.):
- `pr::query::{list(client, &PrFilter) -> Vec<PullRequest>, get(client, id) -> PullRequest, checks(client, id) -> Vec<CommitStatus>}`
- `pr::actions::{approve, unapprove, merge, decline, comment, checkout}` (checkout takes `&dyn GitClient`)
- `issue::query::{list, get}`, `issue::actions::{create, comment}`
- `pipeline::query::{list, get, steps}`
- `repo::query::{get, list}`
- A `PrFilter`/`IssueFilter` struct carrying state/limit/base/author so a TUI
  "section" and the CLI flags both build one.

## Bitbucket endpoint(s)
Unchanged — the exact paths the current commands already hit. The service fns own
path-building + `paginate`; commands stop building paths inline.

## Behavior & edge cases
- Each command's `run()` becomes: resolve repo+auth → build client → call service →
  render (table/TSV/JSON). Error mapping (AuthError/FlagError) preserved.
- No new network behavior, no new output. Pure move + dedup.

## Test cases
- Every existing command test stays green unchanged (the regression guard).
- New unit tests on the service fns with `FakeTransport`: `pr::query::list` builds the
  right path + paginates; `pr::actions::approve` POSTs to the approve endpoint;
  `merge`/`decline` hit the right verbs.

## Staged delivery
The PR domain is the TUI MVP foundation (MVP = #80→#85, all PR), so it lands
first and establishes the pattern: `pr::query` (`PrFilter`, `list`, `checks`) +
`pr::actions` (`approve`, `unapprove`, `merge`, `decline`, `comment`), with the PR
commands refactored to delegate. The `issue`/`pipeline`/`repo` query+actions
extraction follows alongside their TUI section issues (#86 issues, #87 pipelines),
where a second caller actually exercises them — extracting them earlier would be
speculative and add regression surface with no consumer.

## Out of scope
Any `tui/` code. Any change to flags, output, or exit codes.

## Next: spec 034 — TUI scaffold (Epic 9 foundation)
