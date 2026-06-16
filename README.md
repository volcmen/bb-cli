# bb — a Bitbucket CLI

`bb` is a [`gh`](https://github.com/cli/cli)-style command-line tool for
Bitbucket, written in Rust. Authenticate, create and manage pull requests, and
work with repositories from the terminal.

> Status: **early development** (Epic 0 — walking skeleton). See the
> [roadmap issues](https://github.com/volcmen/bb-cli/issues) and `specs/`.

## Install (from source)

```bash
cargo install --path crates/bb
# or
cargo build --release   # binary at target/release/bb
```

## Quickstart

```bash
bb auth login                 # authenticate (App password / API token / OAuth)
bb auth status                # who am I?
bb pr create -t "Title" -b "Body"   # open a PR for the current branch
bb pr list                    # list open PRs for the current repo
```

## Authentication

`bb` supports three credential types on Bitbucket Cloud:

- **Atlassian API token** — `bb auth login --auth-type api_token` (username = your account email).
- **App password** — `bb auth login --auth-type app_password`.
- **OAuth 2.0** — `bb auth login --web` (browser, PKCE). Release binaries that
  embed an OAuth consumer log in out of the box; the callback is a loopback
  `http://127.0.0.1/<random-port>/callback` (RFC 8252, so the consumer's callback
  is just `http://127.0.0.1/callback`). Source builds without an embedded consumer
  need one: register it at `https://bitbucket.org/<workspace>/workspace/settings/api`
  (callback `http://127.0.0.1/callback`), then pass `--client-id`/`--client-secret`,
  export `BB_OAUTH_CLIENT_ID`/`BB_OAUTH_CLIENT_SECRET`, or bake them in at build time
  (set those two env vars when running `cargo build` — `build.rs` embeds them). bb
  stores the consumer after the first login, so later `bb auth login --web` just works.

Pipe a token non-interactively with `--with-token`:

```bash
printf '%s' "$TOKEN" | bb auth login --auth-type api_token --username me@example.com --with-token
```

Credentials are stored in `~/.config/bb/hosts.toml` (`0600`). `BB_TOKEN` / `BB_HOST`
override the stored config.

## Architecture

A Cargo workspace mirroring `gh`'s separation of concerns:

| Crate | Responsibility |
|-------|----------------|
| `bb` | binary: clap command tree, command implementations, exit-code mapping |
| `bb-core` | kernel: DI seam traits, shared types, terminal IO, `Context`, errors |
| `bb-api` | Bitbucket REST client, models, pagination, transport |
| `bb-config` | config + credential storage (`config.toml`, `hosts.toml`) |
| `bb-git` | git shell-out + Bitbucket remote-URL parsing |

Commands depend only on the seam traits, so every command is testable by
injecting fakes (`FakeTransport`, `StubRunner`, `IoStreams::test()`).

## Development

```bash
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all --check
```

Spec-driven / TDD: each issue starts from a spec in `specs/`, gets failing tests,
then an implementation. See `specs/README.md`.

## License

MIT
