//! `bb alias` — user-defined command shorthands (parity with `gh alias`).
//!
//! Aliases are stored as a single JSON object under the `config.toml` global key
//! `aliases`. [`expand`] rewrites the process argv before clap dispatch (called
//! from `main`): `bb co 123` with `co = "pr checkout"` becomes
//! `bb pr checkout 123`. An expansion beginning with `!` is a shell command.

use std::collections::BTreeMap;

use crate::core::{ConfigProvider, Context, FlagError};
use clap::{Args, Subcommand};

#[derive(Args, Debug)]
pub struct AliasArgs {
    #[command(subcommand)]
    command: AliasCommands,
}

#[derive(Subcommand, Debug)]
enum AliasCommands {
    /// Define an alias: `bb alias set co "pr checkout"`
    Set(SetArgs),
    /// List defined aliases
    List,
    /// Delete an alias
    Delete(DeleteArgs),
}

#[derive(Args, Debug)]
pub struct SetArgs {
    /// The alias name (e.g. `co`)
    pub name: String,
    /// What it expands to (e.g. `pr checkout`; prefix with `!` for a shell alias)
    pub expansion: String,
}

#[derive(Args, Debug)]
pub struct DeleteArgs {
    /// The alias to delete
    pub name: String,
}

/// Dispatch `bb alias <sub>`.
///
/// # Errors
/// [`FlagError`] when setting an alias that shadows a built-in command or
/// deleting an unknown alias; propagates [`ConfigError`](crate::core::ConfigError).
pub fn run(ctx: &Context, args: AliasArgs) -> anyhow::Result<()> {
    match args.command {
        AliasCommands::Set(a) => set(ctx, a),
        AliasCommands::List => list(ctx),
        AliasCommands::Delete(a) => delete(ctx, a),
    }
}

fn set(ctx: &Context, args: SetArgs) -> anyhow::Result<()> {
    if crate::cli::builtin_names().iter().any(|b| b == &args.name) {
        return Err(FlagError::new(format!(
            "\"{}\" is already a bb command; choose another alias name",
            args.name
        ))
        .into());
    }
    let mut map = load_aliases(ctx.config.as_ref());
    map.insert(args.name.clone(), args.expansion.clone());
    save_aliases(ctx.config.as_ref(), &map)?;
    ctx.io
        .println(&format!("✓ Added alias {} → {}", args.name, args.expansion));
    Ok(())
}

fn list(ctx: &Context) -> anyhow::Result<()> {
    let map = load_aliases(ctx.config.as_ref());
    if map.is_empty() {
        ctx.io.println("no aliases set");
        return Ok(());
    }
    for (name, expansion) in &map {
        ctx.io.println(&format!("{name}: {expansion}"));
    }
    Ok(())
}

fn delete(ctx: &Context, args: DeleteArgs) -> anyhow::Result<()> {
    let mut map = load_aliases(ctx.config.as_ref());
    if map.remove(&args.name).is_none() {
        return Err(FlagError::new(format!("no such alias: {}", args.name)).into());
    }
    save_aliases(ctx.config.as_ref(), &map)?;
    ctx.io.println(&format!("✓ Deleted alias {}", args.name));
    Ok(())
}

/// Read the alias map from config (empty when unset/malformed).
#[must_use]
pub fn load_aliases(config: &dyn ConfigProvider) -> BTreeMap<String, String> {
    config
        .get("", "aliases")
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_aliases(config: &dyn ConfigProvider, map: &BTreeMap<String, String>) -> anyhow::Result<()> {
    let json = serde_json::to_string(map)?;
    config.set("", "aliases", &json)?;
    config.save()?;
    Ok(())
}

/// The outcome of attempting alias expansion on the process argv.
#[derive(Debug, PartialEq, Eq)]
pub enum Expanded {
    /// Parse these argv tokens with clap (`[0]` is the program name).
    Clap(Vec<String>),
    /// Execute this shell command line (a `!`-alias).
    Shell(String),
}

/// Expand a leading alias in `argv` (`[0]` = program name). Built-in subcommands
/// and flags are never treated as aliases, and expansion is not recursive.
#[must_use]
pub fn expand(
    argv: &[String],
    builtins: &[String],
    aliases: &BTreeMap<String, String>,
) -> Expanded {
    let Some(name) = argv.get(1) else {
        return Expanded::Clap(argv.to_vec());
    };
    if name.starts_with('-') || builtins.iter().any(|b| b == name) {
        return Expanded::Clap(argv.to_vec());
    }
    let Some(expansion) = aliases.get(name) else {
        return Expanded::Clap(argv.to_vec());
    };

    let rest = &argv[2..];
    if let Some(shell) = expansion.strip_prefix('!') {
        let mut line = shell.to_owned();
        for arg in rest {
            line.push(' ');
            line.push_str(&shell_quote(arg));
        }
        return Expanded::Shell(line);
    }

    let mut out = Vec::with_capacity(rest.len() + 4);
    out.push(argv[0].clone());
    out.extend(tokenize(expansion));
    out.extend(rest.iter().cloned());
    Expanded::Clap(out)
}

/// Single-quote `arg` for safe inclusion in a `sh -c` line.
fn shell_quote(arg: &str) -> String {
    format!("'{}'", arg.replace('\'', r"'\''"))
}

/// Split an alias expansion into tokens, honoring single/double quotes (with
/// backslash escapes inside double quotes).
fn tokenize(s: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let mut has = false;
    let mut single = false;
    let mut double = false;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' if !double => {
                single = !single;
                has = true;
            }
            '"' if !single => {
                double = !double;
                has = true;
            }
            '\\' if double => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            c if c.is_whitespace() && !single && !double => {
                if has {
                    tokens.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            c => {
                cur.push(c);
                has = true;
            }
        }
    }
    if has {
        tokens.push(cur);
    }
    tokens
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::api::testing::FakeTransport;
    use crate::config::FileConfig;
    use crate::core::{ConfigProvider, GitClient, Transport};
    use crate::git::{ShellGit, StubRunner};

    use super::*;
    use crate::testsupport::{test_context, ScriptedPrompter};

    fn p(s: &str) -> String {
        s.to_owned()
    }

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| (*s).to_owned()).collect()
    }

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (p(k), p(v))).collect()
    }

    const BUILTINS: &[&str] = &["pr", "repo", "alias", "auth"];
    fn builtins() -> Vec<String> {
        BUILTINS.iter().map(|s| p(s)).collect()
    }

    // ----- expansion (pure) ----------------------------------------------

    #[test]
    fn expand_simple_alias_splices_args() {
        let out = expand(
            &argv(&["bb", "co", "123"]),
            &builtins(),
            &map(&[("co", "pr checkout")]),
        );
        assert_eq!(out, Expanded::Clap(argv(&["bb", "pr", "checkout", "123"])));
    }

    #[test]
    fn expand_quoted_expansion_tokenizes() {
        let out = expand(
            &argv(&["bb", "new"]),
            &builtins(),
            &map(&[("new", "pr create --title \"a b\"")]),
        );
        assert_eq!(
            out,
            Expanded::Clap(vec![p("bb"), p("pr"), p("create"), p("--title"), p("a b"),])
        );
    }

    #[test]
    fn expand_shell_alias_quotes_user_args() {
        let out = expand(
            &argv(&["bb", "prs", "open now"]),
            &builtins(),
            &map(&[("prs", "!bb pr list | grep")]),
        );
        assert_eq!(out, Expanded::Shell(p("bb pr list | grep 'open now'")));
    }

    #[test]
    fn expand_builtin_not_shadowed() {
        // An alias named like a builtin is ignored in favor of the builtin.
        let out = expand(
            &argv(&["bb", "pr", "list"]),
            &builtins(),
            &map(&[("pr", "issue list")]),
        );
        assert_eq!(out, Expanded::Clap(argv(&["bb", "pr", "list"])));
    }

    #[test]
    fn expand_unknown_passthrough() {
        let out = expand(&argv(&["bb", "nope", "x"]), &builtins(), &map(&[]));
        assert_eq!(out, Expanded::Clap(argv(&["bb", "nope", "x"])));
    }

    #[test]
    fn expand_flag_and_empty_passthrough() {
        assert_eq!(
            expand(
                &argv(&["bb", "-R", "a/b"]),
                &builtins(),
                &map(&[("co", "x")])
            ),
            Expanded::Clap(argv(&["bb", "-R", "a/b"]))
        );
        assert_eq!(
            expand(&argv(&["bb"]), &builtins(), &map(&[])),
            Expanded::Clap(argv(&["bb"]))
        );
    }

    #[test]
    fn tokenize_handles_quotes() {
        assert_eq!(tokenize("a  b"), vec![p("a"), p("b")]);
        assert_eq!(tokenize("'x y' z"), vec![p("x y"), p("z")]);
        assert_eq!(
            tokenize(r#"--m "a \"q\" b""#),
            vec![p("--m"), p("a \"q\" b")]
        );
    }

    // ----- the command ----------------------------------------------------

    fn ctx_with(cfg: Arc<dyn ConfigProvider>) -> (Context, crate::core::TestBuffers) {
        let transport: Arc<dyn Transport> = Arc::new(FakeTransport::new());
        let git: Arc<dyn GitClient> = Arc::new(ShellGit::new(Arc::new(StubRunner::new())));
        test_context(
            transport,
            git,
            cfg,
            Arc::new(ScriptedPrompter::new()),
            false,
        )
    }

    fn temp_cfg() -> (Arc<FileConfig>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        (
            Arc::new(FileConfig::load_from(dir.path().to_path_buf()).unwrap()),
            dir,
        )
    }

    #[test]
    fn set_then_load_roundtrips() {
        let (cfg, _d) = temp_cfg();
        let (ctx, bufs) = ctx_with(cfg.clone());
        run(
            &ctx,
            AliasArgs {
                command: AliasCommands::Set(SetArgs {
                    name: p("co"),
                    expansion: p("pr checkout"),
                }),
            },
        )
        .unwrap();
        assert!(bufs
            .stdout_string()
            .contains("✓ Added alias co → pr checkout"));
        assert_eq!(
            load_aliases(cfg.as_ref()).get("co").map(String::as_str),
            Some("pr checkout")
        );
    }

    #[test]
    fn set_rejects_builtin_name() {
        let (cfg, _d) = temp_cfg();
        let (ctx, _bufs) = ctx_with(cfg);
        let err = run(
            &ctx,
            AliasArgs {
                command: AliasCommands::Set(SetArgs {
                    name: p("pr"),
                    expansion: p("issue list"),
                }),
            },
        )
        .unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
    }

    #[test]
    fn delete_removes_and_missing_errors() {
        let (cfg, _d) = temp_cfg();
        cfg.set("", "aliases", r#"{"co":"pr checkout"}"#).unwrap();
        let (ctx, _bufs) = ctx_with(cfg.clone());
        run(
            &ctx,
            AliasArgs {
                command: AliasCommands::Delete(DeleteArgs { name: p("co") }),
            },
        )
        .unwrap();
        assert!(load_aliases(cfg.as_ref()).is_empty());

        let err = run(
            &ctx,
            AliasArgs {
                command: AliasCommands::Delete(DeleteArgs { name: p("nope") }),
            },
        )
        .unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
    }

    #[test]
    fn list_reports_aliases_and_empty() {
        let (cfg, _d) = temp_cfg();
        let (ctx, bufs) = ctx_with(cfg.clone());
        run(
            &ctx,
            AliasArgs {
                command: AliasCommands::List,
            },
        )
        .unwrap();
        assert!(bufs.stdout_string().contains("no aliases set"));

        cfg.set("", "aliases", r#"{"co":"pr checkout"}"#).unwrap();
        let (ctx2, bufs2) = ctx_with(cfg);
        run(
            &ctx2,
            AliasArgs {
                command: AliasCommands::List,
            },
        )
        .unwrap();
        assert!(bufs2.stdout_string().contains("co: pr checkout"));
    }
}
