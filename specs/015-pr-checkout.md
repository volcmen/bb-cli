# 015 bb pr checkout

## Goal
Check out a pull request's source branch locally.

## Command surface
`bb pr checkout ID`. Exit 0; 4 AuthError; 1 invalid/git failure.

## Endpoint + git
- `GET .../pullrequests/{id}` → `source.branch.name` (+ `source.repository` for cross-fork).
- Same-repo: `git fetch origin <branch>` then `git checkout <branch>`.
- Cross-fork: `git remote add <fork> <url>` (if missing), fetch, checkout.

## Behavior & edge cases
- Uses `GitClient::fetch/checkout/add_remote`.
- Cross-fork source detection via `source.repository.full_name` vs base repo.

## Tests
same-repo checkout (StubRunner asserts fetch+checkout); not-found; not-authed → AuthError.

## Next: Epic 2 — repo commands (specs 016+)
