# 031 bb completion

## Goal
Generate shell completion scripts (the `gh completion` analog). First slice of
Epic 8 (packaging) — pure, offline, no network/auth.

## Command surface
`bb completion [-s | --shell <bash|zsh|fish|powershell|elvish>]`. Exit 0; exit 1
(`FlagError`) when `--shell` is omitted on an interactive stdout.

## Behavior & edge cases
- Shell type is `clap_complete::Shell` (clap `ValueEnum`); script generated from
  the full `Cli` command tree via `clap_complete::generate`, written to stdout.
- Shell resolution mirrors `gh`: explicit `--shell` wins; if omitted, default to
  `bash` when stdout is **not** a TTY (piped/eval), but **require** the flag when
  stdout **is** a TTY (avoids dumping a script into the user's terminal).
- No auth, no Context network use — only `ctx.io` for output + the stdout-TTY check.

## Tests
- `generate(Bash)` emits a non-empty script mentioning the `bb` binary.
- `run` non-TTY + no `--shell` → defaults to bash, prints a script.
- `run` TTY + no `--shell` → `FlagError` ("--shell is required").
- `run` explicit `--shell zsh` on a TTY → prints (flag overrides the TTY guard).
- cli parse: `bb completion -s fish` parses; an invalid shell value is a parse error.

## Out of scope
cargo-dist release workflow, Homebrew tap, man pages, the release-time OAuth
consumer (#52) — later Epic 8 slices.

## Next: Epic 8 — man pages (`clap_mangen`) then cargo-dist release automation.
