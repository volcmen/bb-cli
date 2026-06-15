# 004 Git remote → RepoId

## Goal
Resolve the current repo from git remotes (or `-R/--repo`).

## API
- `GitClient` seam: `current_branch`, `remotes`, `push`, `commits_between`.
- `ShellGit` shells out via injectable `CommandRunner` (`RealRunner` / `StubRunner`).
- `parse_remote_url(&str) -> Option<RepoId>`.
- `Context::base_repo()`: `--repo` override else best-priority Bitbucket remote.

## Behavior & edge cases
- Parses scp (`git@bitbucket.org:ws/slug.git`), `ssh://…:7999/…`, `https://[user@]…`, no `.git`, trailing `/`; strips userinfo.
- Host check: exact `bitbucket.org` or `*.bitbucket.org` (Cloud only); others → `None`.
- Remote priority: origin < upstream < other; dedup fetch/push lines.

## Tests
URL table (incl. rejections + garbage); remotes dedup/priority/skip-non-bitbucket; current_branch trim; commits_between parse; push args.

## Next: spec 005 — auth login (#14)
