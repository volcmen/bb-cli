//! `bb config` — get/set local configuration values.

use crate::core::{Context, FlagError};
use clap::{Args, Subcommand};

/// Config keys bb recognizes (stored in the global section of `config.toml`).
const KNOWN_KEYS: &[&str] = &["git_protocol", "editor", "pager", "prompt"];

#[derive(Args, Debug)]
pub struct ConfigArgs {
    #[command(subcommand)]
    command: ConfigCommands,
}

#[derive(Subcommand, Debug)]
enum ConfigCommands {
    /// Print a config value
    Get(GetArgs),
    /// Set a config value
    Set(SetArgs),
}

#[derive(Args, Debug)]
pub struct GetArgs {
    /// Config key
    #[arg(value_name = "KEY")]
    pub key: String,
}

#[derive(Args, Debug)]
pub struct SetArgs {
    /// Config key
    #[arg(value_name = "KEY")]
    pub key: String,
    /// Value to store
    #[arg(value_name = "VALUE")]
    pub value: String,
}

/// Run `bb config <sub>`.
///
/// # Errors
/// [`FlagError`] (1) for an unknown key, an invalid value, or a `get` of an
/// unset key; propagates [`ConfigError`](crate::core::ConfigError) on save.
pub fn run(ctx: &Context, args: ConfigArgs) -> anyhow::Result<()> {
    match args.command {
        ConfigCommands::Get(a) => get(ctx, &a.key),
        ConfigCommands::Set(a) => set(ctx, &a.key, &a.value),
    }
}

fn validate_key(key: &str) -> Result<(), FlagError> {
    if KNOWN_KEYS.contains(&key) {
        Ok(())
    } else {
        Err(FlagError::new(format!(
            "unknown config key {key:?}; valid keys: {}",
            KNOWN_KEYS.join(", ")
        )))
    }
}

fn get(ctx: &Context, key: &str) -> anyhow::Result<()> {
    validate_key(key)?;
    match ctx.config.get("", key) {
        Some(v) => {
            ctx.io.println(&v);
            Ok(())
        }
        None => Err(FlagError::new(format!("no value set for config key {key:?}")).into()),
    }
}

fn set(ctx: &Context, key: &str, value: &str) -> anyhow::Result<()> {
    validate_key(key)?;
    if key == "git_protocol" && value != "ssh" && value != "https" {
        return Err(FlagError::new(format!(
            "invalid git_protocol {value:?}; must be \"ssh\" or \"https\""
        ))
        .into());
    }
    ctx.config.set("", key, value)?;
    ctx.config.save()?;
    ctx.io.println(&format!("✓ Set {key} = {value}"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::api::testing::FakeTransport;
    use crate::config::FileConfig;
    use crate::core::{ConfigProvider, FlagError, GitClient, Transport};
    use crate::git::{ShellGit, StubRunner};

    use super::*;
    use crate::testsupport::{test_context, ScriptedPrompter};

    fn git() -> Arc<dyn GitClient> {
        Arc::new(ShellGit::new(Arc::new(StubRunner::new())))
    }

    /// A context with a real (temp-backed) `FileConfig` so `set` can `save()`.
    fn ctx() -> (Context, crate::core::TestBuffers, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let cfg = FileConfig::load_from(dir.path().to_path_buf()).unwrap();
        let config: Arc<dyn ConfigProvider> = Arc::new(cfg);
        let transport: Arc<dyn Transport> = Arc::new(FakeTransport::new());
        let (c, bufs) = test_context(
            transport,
            git(),
            config,
            Arc::new(ScriptedPrompter::new()),
            false,
        );
        (c, bufs, dir)
    }

    fn get_args(key: &str) -> ConfigArgs {
        ConfigArgs {
            command: ConfigCommands::Get(GetArgs {
                key: key.to_owned(),
            }),
        }
    }
    fn set_args(key: &str, value: &str) -> ConfigArgs {
        ConfigArgs {
            command: ConfigCommands::Set(SetArgs {
                key: key.to_owned(),
                value: value.to_owned(),
            }),
        }
    }

    #[test]
    fn config_set_then_get_roundtrip() {
        let (c, bufs, _dir) = ctx();
        run(&c, set_args("git_protocol", "ssh")).unwrap();
        run(&c, get_args("git_protocol")).unwrap();
        let out = bufs.stdout_string();
        assert!(out.contains("✓ Set git_protocol = ssh"), "out: {out}");
        assert!(
            out.trim_end().ends_with("ssh"),
            "get should print ssh: {out}"
        );
    }

    #[test]
    fn config_get_unset_is_flag_error() {
        let (c, _b, _dir) = ctx();
        let err = run(&c, get_args("editor")).unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
    }

    #[test]
    fn config_unknown_key_is_flag_error() {
        let (c, _b, _dir) = ctx();
        let err = run(&c, get_args("bogus")).unwrap_err();
        assert!(err.to_string().contains("valid keys"), "got: {err}");
    }

    #[test]
    fn config_invalid_git_protocol_value_is_flag_error() {
        let (c, _b, _dir) = ctx();
        let err = run(&c, set_args("git_protocol", "ftp")).unwrap_err();
        assert!(err.downcast_ref::<FlagError>().is_some(), "got: {err}");
    }
}
