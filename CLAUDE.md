# bb-cli — a Rust `gh` for Bitbucket

`bb` is a `gh`-style CLI for Bitbucket Cloud. **Single published crate `bb-cli`** (binary `bb`) in a one-member Cargo workspace, Rust 2021. The former leaf crates are now modules (`core`/`api`/`config`/`git`), so it ships as one `cargo install bb-cli`.

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

## Architecture (single crate, modular — `crates/bb/src/`)

- `main.rs` — exit-code map; `cli.rs` — clap tree; `commands/**` — one module per command; `factory.rs` — `build_context`; `auth.rs`, `output.rs`, `render.rs`, `refresh.rs` (OAuth refresh-on-401), `prompt.rs`, `browser.rs`.
- `core/` — kernel: DI **seam traits** (`Transport`, `GitClient`, `Prompter`, `Browser`, `ConfigProvider`), shared types (`RepoId`, `HttpRequest`/`HttpResponse`, errors, `ExitCode`), `IoStreams`, `Context`. Referenced as `crate::core::*`.
- `api/` — Bitbucket REST: `BitbucketClient` (`get`/`post`/`send_empty`/`get_raw`/`paginate`), `models`, `ReqwestTransport`, `#[cfg(test)] testing::FakeTransport`. `crate::api::*`.
- `config.rs` — `config.toml` + `hosts.toml` (creds, written 0600), `EnvConfig` env override. `crate::config::*`.
- `git/` — shell-out `GitClient` + Bitbucket remote-URL→`RepoId` parsing + `#[cfg(test)] StubRunner`. `crate::git::*`.

The absorbed modules carry `#![allow(dead_code)]` (retained seam/model API surface). Commands depend only on the seam traits, so everything is testable by injecting fakes.

## Conventions

- **Commands**: clap `Args` struct + `pub fn run(ctx: &Context, args) -> anyhow::Result<()>`. Build a client with `BitbucketClient::new(ctx.transport.clone(), crate::auth::header_for(ctx.config.as_ref(), &host))`. Resolve the repo with `ctx.base_repo()`.
- **Errors → exit codes** (mapped in `main.rs`): `FlagError`→1, `SilentError`→1, `CancelError`→2, `AuthError`→4. Return `crate::core::AuthError::new(host)` when not authenticated; `FlagError::new(msg)` for usage errors.
- **TDD, spec-driven**: each issue gets a spec in `specs/NNN-<slug>.md` first, then failing tests, then the implementation. As Definition of Done, write the next spec stub and open the next GitHub issue.
- **Bitbucket Cloud only** for now (`api.bitbucket.org/2.0`). Data Center is a later epic behind a host abstraction.

## Testing

- Command logic tests live in `#[cfg(test)] mod tests` **inside the command module** — `bb` is a binary crate, so `tests/` integration files can't see internals (only `bb/tests/cli.rs` black-box smoke tests use the binary).
- Use `crate::testsupport::{ScriptedPrompter, test_context, RecordingBrowser}`, `crate::api::testing::FakeTransport` (Drop-asserts all stubs hit), `crate::git::StubRunner`, and `crate::config::FileConfig`.
- **IMPORTANT: never call `config.save()` on `FileConfig::blank()` in a test** — its dir is empty and it would error/write to CWD. Use `FileConfig::load_from(tempfile::tempdir())` for any test that triggers a save.

## Repo etiquette

- Branch per epic: `epic-N-<slug>`. One PR per epic into `main`; CI (fmt/clippy `-D warnings`/test matrix) must be green.
- Commit messages: conventional style; reference issues; end with `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.
- `references/` (gitignored) holds the `gh` Go source for reference only — never commit it.

## Parallel agent teams

When fanning out, give each agent **disjoint files** (usually one command module each) and freeze shared contracts (cli wiring, `finder.rs`, `bb-api`/`bb-core` signatures) before spawning. Agents must not edit `Cargo.toml`, `cli.rs`, `factory.rs`, `testsupport.rs`, or another team's files. Run an adversarial reviewer subagent on the diff before finalizing; fix only correctness/requirement gaps.
