<div align="center">

# `bb` — a Bitbucket CLI

**A fast, `gh`-style command-line tool for Bitbucket Cloud, written in Rust.**

Authenticate, manage pull requests, browse repos, run pipelines, and script the
Bitbucket API — without leaving your terminal.

[![CI](https://github.com/volcmen/bb-cli/actions/workflows/ci.yml/badge.svg)](https://github.com/volcmen/bb-cli/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/bb-cli.svg)](https://crates.io/crates/bb-cli)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![MSRV](https://img.shields.io/badge/rustc-1.74+-orange.svg)](https://www.rust-lang.org)
![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-lightgrey.svg)

</div>

```console
$ bb pr list
ID  TITLE                          BRANCH                 STATE
42  Add retry to the upload path   fix/upload->main       OPEN
41  Bump ratatui to 0.29           chore/ratatui->main    OPEN
39  Wire the dashboard config      feat/dash->main        OPEN

$ bb pr create -t "Fix flaky upload test" -b "Closes #42"
https://bitbucket.org/acme/widgets/pull-requests/43
```

---

## Contents

- [Why `bb`](#why-bb)
- [Features](#features)
- [Installation](#installation)
- [Quickstart](#quickstart)
- [Authentication](#authentication)
- [Command reference](#command-reference)
- [Scripting: `--json` / `--jq` / `--template`](#scripting---json----jq----template)
- [Interactive dashboard (`bb dash`)](#interactive-dashboard-bb-dash)
- [Coming from `gh`?](#coming-from-gh)
- [Configuration](#configuration)
- [Architecture](#architecture)
- [Development](#development)
- [License](#license)

## Why `bb`

GitHub has [`gh`](https://github.com/cli/cli). Bitbucket Cloud didn't have an
equivalent that feels native to terminal workflows — so `bb` is that tool:

- **Familiar.** If you know `gh`, you already know `bb` — the same verbs, flags,
  and `-R owner/repo` override.
- **Fast & self-contained.** A single statically-linked Rust binary. No runtime,
  no Python, no Node. `forbid(unsafe_code)`.
- **Scriptable.** First-class `--json`, a built-in `jq` engine (`--jq`), and
  `--template` (tinytemplate) output on every list/view command.
- **Safe by default.** Credentials stored `0600`, OAuth with PKCE + loopback
  redirect, transparent token refresh on `401`, and no secrets in logs.

## Features

- 🔐 **Auth** — Atlassian API token, app password, or OAuth 2.0 (`--web`, PKCE)
  with seamless background token refresh.
- 🔀 **Pull requests** — create, list, view, diff, edit, comment, review/approve,
  merge, close, check out locally, and inspect CI checks.
- 📦 **Repositories** — view, create, clone, fork, rename, delete, set a default,
  sync a fork, plus admin: webhooks, deploy keys, branch restrictions, default reviewers.
- 🐛 **Issues** — list, view, create, comment, edit, close, reopen.
- 🚦 **Pipelines** — list, view, run, and stop CI pipelines.
- 🧰 **Plumbing** — `bb api` for raw authenticated REST calls, `bb search`
  (repos / code / PRs), Pipelines variables, SSH keys, snippets, and workspaces.
- 🖥️ **Interactive dashboard** — `bb dash`, a `ratatui` TUI for triaging PRs,
  issues, and pipelines with vim-style keys.
- 🧩 **Shell completions & man pages** — `bb completion <shell>` and `bb man`.
- 📤 **Machine-readable output** — `--json`, `--jq`, and `--template` everywhere.

## Installation

**From crates.io** — the easiest way:

```bash
cargo install bb-cli                  # installs the `bb` binary
```

**From source:**

```bash
# Install straight from the repo:
cargo install --git https://github.com/volcmen/bb-cli bb-cli

# …or clone and build:
git clone https://github.com/volcmen/bb-cli && cd bb-cli
cargo install --path crates/bb        # installs `bb`
cargo build --release                 # or just build → target/release/bb
```

Requires Rust **1.74+**. `bb` runs on macOS, Linux, and Windows.

## Quickstart

```bash
bb auth login                        # authenticate (API token / app password / OAuth)
bb auth status                       # who am I?

cd your-bitbucket-repo               # bb infers the repo from the git remote
bb pr create -t "Title" -b "Body"    # open a PR for the current branch
bb pr list                           # open PRs for this repo
bb pr view 42 --web                  # open PR #42 in the browser
bb pr checks 42                      # CI status for a PR

bb dash                              # launch the interactive dashboard
```

Run any command with `--help` for its full flag set. `-R, --repo WORKSPACE/SLUG`
targets another repository without `cd`-ing into it.

## Authentication

`bb` supports three credential types on Bitbucket Cloud:

| Method | Command | Notes |
|--------|---------|-------|
| **Atlassian API token** | `bb auth login --auth-type api_token` | username = your account email |
| **App password** | `bb auth login --auth-type app_password` | classic Bitbucket app password |
| **OAuth 2.0** | `bb auth login --web` | browser + PKCE, loopback redirect (RFC 8252) |

Release binaries can embed an OAuth consumer so `--web` works out of the box. A
source build has none, so for `--web` you supply your own consumer — register it
at `https://bitbucket.org/<workspace>/workspace/settings/api` (callback
`http://127.0.0.1/callback`), then pass `--client-id`/`--client-secret`, export
`BB_OAUTH_CLIENT_ID`/`BB_OAUTH_CLIENT_SECRET`, or bake them in at build time
(`build.rs` reads those env vars). `bb` stores the consumer after the first login.

Pipe a token in non-interactively for CI:

```bash
printf '%s' "$TOKEN" | bb auth login --auth-type api_token --username me@example.com --with-token
```

Credentials are written to `~/.config/bb/hosts.toml` with `0600` permissions.
`BB_TOKEN` and `BB_HOST` override the stored config for one-off or CI use.

## Command reference

| Group | Subcommands |
|-------|-------------|
| `bb auth` | `login` · `status` · `logout` · `token` · `setup-git` · `refresh` · `switch` |
| `bb pr` | `create` · `list` · `view` · `diff` · `edit` · `comment` · `review` · `approve` · `merge` · `close` · `checkout` · `checks` · `status` |
| `bb repo` | `view` · `create` · `clone` · `fork` · `edit` · `rename` · `delete` · `list` · `set-default` · `sync` · `webhook` · `deploy-key` · `branch-restriction` · `default-reviewer` |
| `bb issue` | `list` · `view` · `create` · `comment` · `edit` · `close` · `reopen` |
| `bb pipeline` | `list` · `view` · `run` · `stop` |
| `bb variable` | `list` · `set` · `delete` |
| `bb ssh-key` | `list` · `add` · `delete` |
| `bb snippet` | `create` · `list` · `view` · `edit` · `delete` · `clone` |
| `bb workspace` | `list` · `members` · `projects` |
| `bb alias` | `set` · `list` · `delete` |
| `bb config` | `get` · `set` |
| `bb search` | `repos` · `code` · `prs` |
| `bb api` | raw authenticated REST request (`-X`, `-f`, `-F`, paginated) |
| `bb browse` | open a repo or PR in the browser |
| `bb dash` | interactive TUI dashboard |
| `bb completion` / `bb man` | shell completions / man pages |

## Scripting: `--json` / `--jq` / `--template`

Every list/view command can emit structured output, so `bb` drops cleanly into
scripts and pipelines:

```bash
# Raw JSON with selected fields
bb pr list --json id,title,state

# Filter with the built-in jq engine (no external jq needed; implies --json)
bb pr list --jq '.[] | select(.title | test("WIP"))'

# Format with tinytemplate. A top-level array is exposed under `items`;
# interpolate with single braces, loop with double braces:
bb pr list --template '{{ for p in items }}#{ p.id } { p.title }
{{ endfor }}'

# Escape hatch: any REST endpoint, authenticated. On GET, -f adds a query param;
# --paginate concatenates every page's values into one array.
bb api /repositories/acme/widgets/pullrequests -f state=MERGED --paginate
```

## Interactive dashboard (`bb dash`)

`bb dash` opens a [`ratatui`](https://ratatui.rs) terminal UI for triaging work
without juggling commands:

- Tabbed sections for **pull requests**, **issues**, and **pipelines** (with live auto-refresh).
- Vim-style navigation, `/` fuzzy filter, and inline actions (approve, comment, merge).
- Configurable sections, theme, and **custom keybindings** that shell out to your own commands.
- Press `?` for the auto-generated keymap help.

Configuration lives under flat `dash_*` keys in `config.toml` (see below).

## Coming from `gh`?

`bb` mirrors `gh`'s ergonomics — most muscle memory carries over:

| `gh` | `bb` |
|------|------|
| `gh auth login` | `bb auth login` |
| `gh pr create` | `bb pr create` |
| `gh pr list -L 20` | `bb pr list -L 20` |
| `gh pr checkout 42` | `bb pr checkout 42` |
| `gh repo clone o/r` | `bb repo clone o/r` |
| `gh api ...` | `bb api ...` |
| `gh pr list --json ... --jq ...` | `bb pr list --json ... --jq ...` |

## Configuration

| File | Purpose |
|------|---------|
| `~/.config/bb/hosts.toml` | credentials, written `0600` |
| `~/.config/bb/config.toml` | preferences: default repo, aliases, `dash_*` dashboard settings |

Environment overrides: `BB_TOKEN`, `BB_HOST`, `BB_OAUTH_CLIENT_ID`,
`BB_OAUTH_CLIENT_SECRET`. (Paths follow the platform conventions of
[`etcetera`](https://crates.io/crates/etcetera); the above are the Linux/macOS defaults.)

## Architecture

`bb` ships as a **single crate** (`bb-cli`, binary `bb`) organized into modules
that mirror `gh`'s separation of concerns. Commands depend only on
dependency-injection **seam traits**, so every command is unit-testable by
injecting fakes — no network, no real git, no real filesystem.

| Module | Responsibility |
|--------|----------------|
| `commands/**` | one module per command — clap `Args` + `run(ctx, args)` |
| `core` | kernel: seam traits (`Transport`, `GitClient`, `Prompter`, `Browser`, `ConfigProvider`), shared types, `Context`, errors, exit codes |
| `api` | Bitbucket REST client, models, pagination, `reqwest` transport |
| `config` | `config.toml` + `hosts.toml` storage and env overrides |
| `git` | git shell-out + Bitbucket remote-URL → `RepoId` parsing |
| `tui` | the `bb dash` dashboard (Model-Update-View, panic-safe terminal) |

> Bitbucket **Cloud** only for now. Data Center / Server support is planned behind
> a host abstraction.

## Development

Spec-driven and test-first: each feature starts as a spec in [`specs/`](specs/),
gets failing tests, then an implementation. All four gates must be green before a
change merges:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all --check
cargo build --release
```

Contributions are welcome — open an issue or PR. Please keep the gates green and
follow the existing command/test conventions.

## License

[MIT](LICENSE) © volcmen
