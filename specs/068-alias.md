# 068 alias: `bb alias set/list/delete`

Fixes #112. Parity with `gh alias`.

## Goal / user story
User-defined shorthands: `bb alias set co "pr checkout"` then `bb co 123` runs
`bb pr checkout 123`. Shell aliases (`!`) run an arbitrary command line.

## Command surface
- `bb alias set <name> <expansion>` — define/overwrite. Rejects a `name` that is
  already a built-in subcommand (it could never be reached). `expansion` starting
  with `!` is a shell alias.
- `bb alias list` — print `name: expansion`, sorted (or "no aliases set").
- `bb alias delete <name>` — remove; `FlagError` when absent.

## Storage
A single `config.toml` global key `aliases` holding a JSON object
`{ "<name>": "<expansion>" }` (avoids enumerating flat keys / a trait change).

## Expansion (before clap dispatch)
`alias::expand(argv, builtins, aliases) -> Expanded`:
- argv[1] missing, a flag (`-…`), or a built-in subcommand → `Clap(argv)` (built-ins
  always win; no recursion).
- argv[1] is an alias:
  - `!rest` → `Shell("rest <quoted user args…>")`.
  - else tokenize the expansion (quote-aware) and splice:
    `[prog] + tokens + argv[2..]` → `Clap`.
- otherwise → `Clap(argv)`.
`main` runs `Clap` via `Cli::parse_from`, or executes `Shell` via `sh -c`
(`cmd /C` on Windows), propagating the child exit code.

## Test cases (red first)
alias command:
- `set_then_list_roundtrips`; `set_rejects_builtin_name`; `delete_removes`;
  `delete_missing_is_flag_error`.
storage/expansion (pure):
- `expand_simple_alias_splices_args` (`co 123` → `pr checkout 123`).
- `expand_quoted_expansion_tokenizes` (`--title "a b"` stays one token).
- `expand_shell_alias_returns_shell` with user args appended/quoted.
- `expand_builtin_not_shadowed`; `expand_unknown_passthrough`;
  `expand_no_subcommand_passthrough`.
- `tokenize` unit (quotes, escapes).

## Out of scope
Recursive alias expansion; per-alias clap validation; Windows shell-quoting
parity beyond best-effort.

## Next: spec 069 — #107 `bb snippet`
