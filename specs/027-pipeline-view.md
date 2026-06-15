# 027 bb pipeline view

## Goal
Show a pipeline's state + steps.

## Command surface
`bb pipeline view BUILD [--log] [--json …]`. Exit 0; 4 AuthError; 1 not-found.

## Endpoint
- `GET /2.0/repositories/{ws}/{slug}/pipelines/{BUILD}` → Pipeline. **VERIFY**: Bitbucket's path uses the pipeline UUID; build-number selectors may need confirming (pass through; if 404, hint to use list).
- Steps: `GET .../pipelines/{BUILD}/steps/` → step values; `--log` → `GET .../steps/{uuid}/log` (raw).

## Behavior & edge cases
- Render build#, state/result, target ref, then each step (name + state/result). `--json` emits the pipeline. not-found → exit 1.

## Tests
view renders state + steps; --json; not-found; not-authed → AuthError.

## Next: spec 028 — pr checks (#42)
