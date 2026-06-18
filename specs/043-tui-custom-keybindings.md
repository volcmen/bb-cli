# 043 tui: custom keybindings → external command (stretch)

## Goal
gh-dash's power feature, with a vim-native feel: bind a key to run a templated
external command seeded with the selected item's context (url, branch, id). Lets
users wire the dashboard into their own tools (lazygit, `$EDITOR`, a review script).

## Command surface
Internal. Config-defined under `[[dash.custom_keys]]`. Pressing the bound key
suspends the TUI (leave alt-screen + raw mode via the spec-034 guard), runs the
command, then resumes on exit.

## Config schema
```toml
[[dash.custom_keys]]
key = "g"                 # only on free keys (validated against the built-in keymap)
name = "lazygit"
command = "lazygit"
context = "pr"            # which view this applies to

[[dash.custom_keys]]
key = "v"
name = "open in editor"
command = "$EDITOR {{branch}}"
```
Template vars: `{{id}}`, `{{url}}`, `{{branch}}`, `{{repo}}`, `{{workspace}}`,
`{{slug}}` — expanded from the selected row.

## Behavior & edge cases
- Suspend → run via the shell → resume; terminal restored cleanly even if the
  command fails. Non-zero exit → toast with the exit code.
- A custom key that collides with a built-in binding → rejected at config load with
  a status-line warning (built-ins win).
- No selection → the binding is a no-op with a hint.

## Test cases
- Template expansion fills vars from a selected PR/issue.
- A colliding custom key is rejected; a free key registers.
- Suspend/resume restores terminal state (guard re-entrancy test).

## Out of scope
Sandboxing the command. Async/background custom commands.

## Next: Epic 9 complete. Follow-ups — Epic 7 (Data Center host abstraction) or TUI
polish (diff viewer, full markdown via `tui-markdown`, mouse support).
