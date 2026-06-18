# 044 output: `--jq` / `--template` imply `--json`

Fixes #76.

## Goal / user story
As a user scripting `bb`, `bb pr list --jq '.[].id'` should filter the JSON
output, not silently print the human table. Today `--jq` / `--template` only
take effect when `--json <fields>` is *also* passed; used alone they are
ignored (the command falls through to the table) because the JSON path is gated
on `--json` being non-empty. The help text even claims `--jq`/`--template`
"implies `--json`", which is currently false.

## Command surface
No new flags. Affects every command that flattens `JsonFlags`
(`pr list/view/checks`, `repo list/view`, `issue list/view`, `pipeline
list/view`):

- `--jq <expr>` alone → JSON mode, full object(s), jq applied.
- `--template <tmpl>` alone → JSON mode, full object(s), template applied.
- `--json a,b` (no jq/template) → unchanged: projected JSON, pretty-printed.
- `--json a -q <expr>` → unchanged: project then jq.
- none of the three → unchanged: human table/text.

Exit codes unchanged (`FlagError`→1 for an unknown `--json` field or invalid
jq/template).

## Bitbucket endpoint(s)
None — pure output-layer change in `crates/bb/src/output.rs`.

## Behavior & edge cases
- `JsonFlags::requested()` returns true when **any** of `json`/`jq`/`template`
  is set (was: only `json` non-empty).
- `project()` with an **empty** field list returns the value unchanged (the full
  object/array) instead of projecting to `{}`. So `--jq`/`--template` with no
  `--json` operate on the full response.
- The old `validate()` guard ("`--jq`/`--template` require `--json <fields>`")
  is removed — it is no longer a misuse.
- Unknown `--json` field still errors (`FlagError`). Invalid jq/template still
  errors.
- Help/doc wording updated: "(implies `--json`; projects all fields)".

## Test cases (red first)
- `requested_true_when_only_jq` / `requested_true_when_only_template`.
- `validate_allows_jq_without_json_fields` (replaces the old
  `validate_jq_requires_json`, which asserted the now-removed error).
- `project_empty_fields_returns_full_value` (array and object).
- `emit_jq_without_json_fields_uses_full_object` (e2e via `IoStreams::test`).
- Command level: `pr list --jq` with no `--json` filters instead of printing the
  table (in `pr/list.rs` tests).

## Out of scope
Adding `--jq`/`--template` to `bb api` (#78) — separate spec. Typed `-F` fields
(#94).

## Next: spec 045 — #114 `pr list --base` drops the `--state` filter
