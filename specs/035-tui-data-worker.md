# 035 tui: data worker thread + async-without-tokio protocol

## Goal
Network calls must never freeze the UI. A background worker thread owns a
`BitbucketClient`; the UI sends typed `Request`s and receives `Response`s over
channels. Spinner while in-flight, errors surface to a status line — no panics, no
blocking the 60fps loop. No tokio: `std::thread` + `std::sync::mpsc`.

## Command surface
Internal. `enum Request { Pr(PrFilter), PrDetail(id), Approve(id), … }`,
`enum Response { Prs(Vec<PullRequest>), PrDetail(Box<PullRequest>), Done(ActionId), Error(String, RequestKind) }`.
The event loop merges three sources: crossterm input, the `Response` receiver, and
a tick timer (drives the spinner + later auto-refresh).

## Architecture
- Worker: `spawn` a thread holding `Arc<dyn Transport>` + auth header → builds the
  service-layer (spec 033) client; loops on the `Request` receiver; sends `Response`.
- UI thread: `App` records in-flight `RequestKind`s for the spinner; consumes
  `Response` and folds it into state via the reducer.
- 401 inside the worker runs the existing refresh-on-401 path; if refresh fails →
  `Response::Error` carrying a "re-authenticate" hint.

## Behavior & edge cases
- Multiple in-flight requests OK; responses matched by `RequestKind`.
- Worker thread death (channel closed) → UI shows a fatal toast and allows quit.
- Slow network → spinner + the rest of the UI stays interactive.

## Test cases
- Reducer: `Response::Prs` clears the loading flag and populates rows;
  `Response::Error` sets the status toast.
- Worker integration with `FakeTransport`: a `Request::Pr` yields `Response::Prs`
  with the expected items.

## Out of scope
Concrete view widgets (036+). Auto-refresh polling (040 uses this tick).

## Next: spec 036 — PR list view
