# 046 api: typed fields (`-F`/`--field`)

Fixes #94.

## Goal / user story
`bb api -X PUT /repositories/WS/SLUG -F has_issues=true` must send
`{"has_issues": true}` (JSON boolean), not `{"has_issues": "true"}` (string).
Today `bb api` only has `-f`, which sends every value as a string, so booleans /
numbers / null cannot be expressed and requests like enabling a repo feature are
silently no-ops.

## Command surface
Align with `gh api`:

- `-f, --raw-field KEY=VALUE` — string value (was `-f, --field`; the long name
  changes to `--raw-field`, the `-f` short and string semantics are unchanged).
- `-F, --field KEY=VALUE` — **typed** value: `true`/`false` → bool, `null` →
  null, integer/float → number; anything else → string.
- Both repeatable; merged into one JSON object. On a duplicate key, `-F` (parsed
  later) wins.
- Exit codes unchanged: malformed `KEY=VALUE` → `FlagError` (1). `--paginate`
  still rejects any body (`-f`/`-F`).

## Bitbucket endpoint(s)
N/A — request-body construction only.

## Behavior & edge cases
- `-F x=true|false|null` → bool/null. `-F n=5` / `-F n=-3` / `-F r=1.5` → number.
- `-F name=foo` (not a JSON literal) → string `"foo"`.
- `-f flag=true` → string `"true"` (raw stays string).
- No fields of either kind → no body (`None`), as today.

## Test cases (red first)
- `typed_field_parses_literals`: `-F b=true -F n=5 -F z=null -F r=1.5` →
  `{"b":true,"n":5,"z":null,"r":1.5}`.
- `typed_field_non_literal_is_string`: `-F name=foo` → `{"name":"foo"}`.
- `raw_field_keeps_string_for_literal`: `-f flag=true` → `{"flag":"true"}`.
- `raw_and_typed_merge`: `-f a=x -F b=true` → `{"a":"x","b":true}`.
- malformed typed field (`-F novalue`) → `FlagError`.

## Out of scope
`@file` / `-` (stdin) field values and a raw `--input` body — note for a later
spec. Adding `--jq`/`--template` to `bb api` (#78).

## Next: spec 047 — #78 `bb api` `--jq` / `--template`
