# 017 bb repo clone

## Goal
Clone a Bitbucket repository.

## Command surface
`bb repo clone WORKSPACE/SLUG [DIRECTORY]`. Exit 0; 4 AuthError; 1 not-found/git failure.

## Endpoint + git
- `GET /2.0/repositories/{ws}/{slug}` → `links.clone[]`.
- Pick clone URL by `git_protocol` config (`ssh` or `https`, default `https`): `repo.clone_url(protocol)`.
- `GitClient::clone_repo(url, dir)` (shells out to `git clone`).

## Behavior & edge cases
- DIRECTORY defaults to the slug (git's default). Not-found → exit 1.
- If preferred protocol URL missing, fall back to the other.

## Tests
clone https (StubRunner asserts `git clone <url> [dir]`); ssh per config; not-found; not-authed → AuthError.

## Next: spec 018 — repo list (#29)
