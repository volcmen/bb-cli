# 047 api: `--jq` / `--template`

Fixes #78.

## Goal / user story
`bb api /user --jq '.display_name'` should filter the response, like `gh api
--jq`. Today `bb api` only pretty-prints the raw JSON; there is no way to project
or template the result, so scripting requires piping to an external `jq`.

## Command surface
`bb api <path> [-q|--jq <expr>] [--template <tmpl>]` — two new output flags
(no `--json <fields>`: the response shape is arbitrary, so jq/template operate on
the **full** body, matching `gh api`). Compatible with `--paginate` (applied to
the combined array). Mutually fine with any method.

## Bitbucket endpoint(s)
N/A — output-layer only. Reuses `output::JsonFlags::emit` (jq via jaq, template
via tinytemplate) with an empty field list (full value).

## Behavior & edge cases
- `--jq`/`--template` set → parse the response body as JSON and emit through
  `JsonFlags { json: [], jq, template }`. An invalid expression/template →
  `FlagError`.
- Response body is not JSON while `--jq`/`--template` is set → `FlagError`
  ("response is not JSON").
- Neither flag → unchanged pretty-print (or raw text for non-JSON).
- `--paginate` + `--jq`/`--template` → filter/template the concatenated array.
- HTTP `>= 400` still returns `SilentError` after emitting.

## Test cases (red first)
- `api_jq_filters_response`: `/user` → `--jq '.username'` prints `"davidd"`.
- `api_template_renders_response`: `--template '{username}'` prints `davidd`.
- `api_jq_on_paginate`: `--paginate --jq '.[].id'` prints the ids.
- `api_jq_non_json_body_is_flag_error`: 200 non-JSON body + `--jq` → `FlagError`.

## Out of scope
`--json <fields>` projection on `bb api`. `@file` field values (spec 046 note).

## Next: spec 048 — #92 `repo clone` over SSH / token (OAuth-safe)
