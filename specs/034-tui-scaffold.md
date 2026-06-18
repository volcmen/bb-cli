# 034 tui: scaffold — `bb dash` + ratatui app loop

## Goal
`bb dash` opens a full-screen, keyboard-driven dashboard. This issue lays the
foundation: dependencies, a panic-safe terminal guard, the App state machine
(Model-Update-View), and clean quit. Renders a placeholder frame; no data yet.

## Command surface
`bb dash` — exit 0 on clean quit. Requires an interactive terminal: if stdout is
not a TTY → `FlagError` "bb dash requires an interactive terminal" (exit 1). Not
authenticated → render a "Not logged in — run `bb auth login`" screen (still a TUI),
not an immediate AuthError. Global `-R/--repo` honored. Wired into `cli.rs`
`Commands::Dash` + dispatch.

## Architecture
- New deps: `ratatui` (crossterm backend), `crossterm`. No tokio/async — keeps the
  blocking-transport invariant.
- New module `crate::tui`: `app.rs` (App model + `update(&mut self, event) ` reducer +
  `view(&self, frame)`), `event.rs` (input loop), `terminal.rs` (RAII guard: enter
  alt-screen + raw mode on new, restore on Drop; install a panic hook that restores
  the terminal before the default hook prints the panic).
- App is the single source of truth: active tab, selection, modal stack, `should_quit`.
- `keymap.rs` establishes the **vim-native keymap grammar** all views obey (see the
  design doc's "Keymap standard"): `h/j/k/l` motion, `g/G` ends, `Ctrl-d/Ctrl-u`
  half-page, `/` + `n/N` search, `:`-style nowhere (no command line), `?` help,
  number tabs `1/2/3`, `Esc` always pops one layer, `q` quits. Bindings are
  data-driven so 042 can override and `?` can render them.

## Behavior & edge cases
- `q` / `Ctrl-C` / `Esc` (at root) → quit, terminal restored.
- Panic mid-render → terminal restored, normal panic message visible (no garbled TTY).
- Terminal resize → re-layout on next frame.

## Test cases
- `update(App, Key('q'))` sets `should_quit`; `Esc` at root quits, in a modal pops it.
- A frame renders through `ratatui::backend::TestBackend` without panicking.
- Non-TTY invocation returns the FlagError; TTY detection via `IoStreams::is_stdout_tty`.

## Out of scope
Any real data fetch (035), any concrete view (036+).

## Next: spec 035 — data worker thread + request/response protocol
