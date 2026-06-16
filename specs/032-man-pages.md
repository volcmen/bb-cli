# 032 bb man

## Goal
Generate roff man pages for `bb` and every subcommand (the cobra `GenManTree`
analog). Second Epic 8 slice — pure, offline.

## Command surface
`bb man -o | --output <DIR>`. Exit 0; `FlagError` if the directory can't be
created/written.

## Behavior & edge cases
- Render with `clap_mangen` from the full `Cli` command tree, recursively:
  the root → `bb.1`, each subcommand → `bb-<path>.1`
  (e.g. `bb-pr.1`, `bb-pr-create.1`, `bb-completion.1`).
- Create `<DIR>` if missing; write one `*.1` file per command; print a summary
  (`Wrote N man pages to <DIR>`).
- No auth/network — only `ctx.io` for the summary line + the filesystem.

## Tests
- `render_pages()` includes `bb.1` and nested entries (`bb-pr.1`,
  `bb-pr-create.1`, `bb-completion.1`); each is non-empty and contains a `.TH`
  roff header.
- `run` writes the expected files into a tempdir and prints the count.
- cli parse: `bb man -o /tmp/x` parses; missing `--output` is a parse error.

## Out of scope
cargo-dist release workflow, Homebrew tap, the release-time OAuth consumer (#52)
— these need decisions/resources and are tracked separately.

## Next: Epic 8 — cargo-dist release automation (needs target/tag decisions) +
## Homebrew tap + the release-time OAuth consumer (#52).
