# 063 pipeline: `bb pipeline run` / `stop`

Fixes #105.

## Goal / user story
Trigger and stop Bitbucket Pipelines from the CLI (gh `run` parity). Reading
step logs is already covered by `bb pipeline view --log`, so this spec adds the
two missing *actions*: run and stop.

## Command surface
- `bb pipeline run [--branch B] [--custom NAME]` — trigger; branch defaults to
  the current branch; `--custom` runs a custom pipeline by name.
- `bb pipeline stop <BUILD>` — stop a running pipeline.

Exit codes: `AuthError`(4); propagates API errors.

## Bitbucket endpoint(s)
- run: `POST …/pipelines/` with
  `{target:{type:"pipeline_ref_target", ref_type:"branch", ref_name, selector?}}`
  (`selector:{type:"custom", pattern:NAME}` for `--custom`).
- stop: `POST …/pipelines/{build}/stopPipeline`.

## Behavior & edge cases
- run prints `✓ Started pipeline #{build_number}` + the results URL.
- stop prints `✓ Stopped pipeline #{build}`.
- `view --log` remains the way to read logs (noted; not duplicated).

## Test cases (red first)
- `run_posts_branch_target` (default branch via git stub).
- `run_explicit_branch`, `run_with_custom_selector` (body has selector).
- `stop_posts_stop_pipeline`.
- `not_authed_is_auth_error`.

## Out of scope
`--tag` / `--commit` targets, `--variable`, watch/follow — follow-up. A
standalone `pipeline logs` (use `view --log`).

## Next: spec 064 — #104 `bb auth logout` / `token` (+ setup-git)
