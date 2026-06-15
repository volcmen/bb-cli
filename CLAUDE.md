# bb-cli — a Rust `gh` for Bitbucket

`bb` is a `gh`-style CLI for Bitbucket Cloud. Cargo workspace, Rust 2021.

## Build / test commands

**IMPORTANT: `cargo` is not on `PATH` in this environment. Prefix every cargo command:**
```bash
export PATH="$HOME/.cargo/bin:$PATH"
```
Then:
```bash
cargo build --workspace
cargo test --workspace                 # or -p <crate> for one crate (faster)
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all                        # --check in CI
```
All four must pass before a commit is considered done. Prefer `-p <crate>` while iterating.

## Architecture (5 crates)

- `bb` — binary: clap tree (`src/cli.rs`), commands (`src/commands/**`), exit-code map (`src/main.rs`), real `Prompter`/`Browser`, `factory::build_context`.
- `bb-core` — kernel: DI **seam traits** (`Transport`, `GitClient`, `Prompter`, `Browser`, `ConfigProvider`), shared types (`RepoId`, `HttpRequest`/`HttpResponse`, errors, `ExitCode`), `IoStreams`, `Context`. Depends on nothing.
- `bb-api` — Bitbucket REST: `BitbucketClient` (`get`/`post`/`send_empty`/`get_raw`/`paginate`), models, `ReqwestTransport`, `FakeTransport` (test harness).
- `bb-config` — `config.toml` + `hosts.toml` (creds), `EnvConfig` env override.
- `bb-git` — shell-out `GitClient` + Bitbucket remote-URL→`RepoId` parsing + `StubRunner`.

Leaf crates depend only on `bb-core`. Commands depend only on the seam traits, so everything is testable by injecting fakes.

## Conventions

- **Commands**: clap `Args` struct + `pub fn run(ctx: &Context, args) -> anyhow::Result<()>`. Build a client with `BitbucketClient::new(ctx.transport.clone(), crate::auth::header_for(ctx.config.as_ref(), &host))`. Resolve the repo with `ctx.base_repo()`.
- **Errors → exit codes** (mapped in `main.rs`): `FlagError`→1, `SilentError`→1, `CancelError`→2, `AuthError`→4. Return `bb_core::AuthError::new(host)` when not authenticated; `FlagError::new(msg)` for usage errors.
- **TDD, spec-driven**: each issue gets a spec in `specs/NNN-<slug>.md` first, then failing tests, then the implementation. As Definition of Done, write the next spec stub and open the next GitHub issue.
- **Bitbucket Cloud only** for now (`api.bitbucket.org/2.0`). Data Center is a later epic behind a host abstraction.

## Testing

- Command logic tests live in `#[cfg(test)] mod tests` **inside the command module** — `bb` is a binary crate, so `tests/` integration files can't see internals (only `bb/tests/cli.rs` black-box smoke tests use the binary).
- Use `crate::testsupport::{ScriptedPrompter, test_context, RecordingBrowser}`, `bb_api::testing::FakeTransport` (Drop-asserts all stubs hit), `bb_git::StubRunner`, and `bb_config::FileConfig`.
- **IMPORTANT: never call `config.save()` on `FileConfig::blank()` in a test** — its dir is empty and it would error/write to CWD. Use `FileConfig::load_from(tempfile::tempdir())` for any test that triggers a save.

## Repo etiquette

- Branch per epic: `epic-N-<slug>`. One PR per epic into `main`; CI (fmt/clippy `-D warnings`/test matrix) must be green.
- Commit messages: conventional style; reference issues; end with `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.
- `references/` (gitignored) holds the `gh` Go source for reference only — never commit it.

## Parallel agent teams

When fanning out, give each agent **disjoint files** (usually one command module each) and freeze shared contracts (cli wiring, `finder.rs`, `bb-api`/`bb-core` signatures) before spawning. Agents must not edit `Cargo.toml`, `cli.rs`, `factory.rs`, `testsupport.rs`, or another team's files. Run an adversarial reviewer subagent on the diff before finalizing; fix only correctness/requirement gaps.
