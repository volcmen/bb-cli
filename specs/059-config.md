# 059 config: `bb config get` / `set`

Fixes #111.

## Goal / user story
Read and write bb's local config from the CLI (parity with the core of `gh
config get|set`). Keys are stored in the global section of `config.toml` (the
same place `git_protocol` already lives, used by `repo clone`).

## Command surface
- `bb config get <KEY>` — print the value, or `FlagError` if unset.
- `bb config set <KEY> <VALUE>` — persist it (`save()`).

Known keys: `git_protocol` (ssh|https), `editor`, `pager`, `prompt`
(enabled|disabled). An unknown key is a `FlagError` listing the valid keys; an
invalid `git_protocol` value is a `FlagError`.

Exit codes: `FlagError`(1) unknown key / invalid value / unset on get.

## Bitbucket endpoint(s)
None — local config only (`ConfigProvider::get`/`set`/`save`, global host `""`).

## Behavior & edge cases
- `set git_protocol ssh` then `get git_protocol` → `ssh`.
- `get` of a known-but-unset key → `FlagError` ("no value set …").
- `get`/`set` of an unknown key → `FlagError` (valid keys listed).
- `set git_protocol bogus` → `FlagError` (value must be ssh|https).

## Test cases (red first)
- `config_set_then_get_roundtrip`.
- `config_get_unset_is_flag_error`.
- `config_unknown_key_is_flag_error`.
- `config_invalid_git_protocol_value_is_flag_error`.

## Out of scope
`config list` (needs a `ConfigProvider` enumerate method — seam change),
per-host config keys.

## Next: spec 060 — #109 `bb ssh-key` (account SSH keys)
