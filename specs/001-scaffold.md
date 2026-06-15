# 001 Scaffold: workspace + bb-core contracts + `bb --version`

## Goal / user story
A compiling Cargo workspace with the kernel contracts in place, a working
`bb --version`, the exit-code/error harness, and CI — the foundation every later
issue builds on.

## Command surface
- `bb --version` → `bb 0.1.0 (<sha> <date>)` (clap), exit 0.
- `bb version` → `bb version 0.1.0 (<sha> <date>)`, exit 0.
- `bb` (no args) → prints help, exit 0.
- unknown command / bad flag → clap prints usage to stderr, exit 2.
- Exit codes: `Ok=0`, `Error=1`, `Cancel=2`, `Auth=4`.

## Architecture
- Workspace crates: `bb` (binary), `bb-core` (kernel: seam traits + types + IO +
  Context + errors), `bb-api`, `bb-config`, `bb-git`.
- Seam traits in `bb-core`: `Transport`, `GitClient`, `Prompter`, `Browser`,
  `ConfigProvider`. Concrete impls in leaf crates / the binary.
- Shared types: `RepoId`, `HttpRequest`/`HttpResponse`/`Method`, `ApiError` and
  command sentinels (`FlagError`, `SilentError`, `AuthError`, `CancelError`),
  `ExitCode`, `Context`, `IoStreams` (+ `test()` constructor), `ColorScheme`.

## Behavior & edge cases
- `build.rs` captures git SHA + commit date; both fall back to `unknown` outside
  a git checkout.
- `IoStreams::test()` returns in-memory buffers; TTY flags overridable.

## Test cases
- `RepoId` parses `WORKSPACE/SLUG` and `HOST/WS/SLUG`; rejects bare slug.
- `IoStreams::test()` captures stdout/stderr; `ColorScheme` is plain when disabled.
- `assert_cmd`: `bb --version` prints a version line, exit 0; unknown command exits 2.

## Out of scope
Any real command (auth, pr) — those are later issues.

## Next: spec 002 — config storage (#11)
