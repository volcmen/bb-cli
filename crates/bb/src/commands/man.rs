//! `bb man` — generate roff man pages for `bb` and every subcommand (the cobra
//! `GenManTree` analog). Pure and offline: it renders the static `clap` command
//! tree to `*.1` files; no network or authentication.

use std::path::PathBuf;

use bb_core::{Context, FlagError};
use clap::{Args, CommandFactory};

#[derive(Args, Debug)]
pub struct ManArgs {
    /// Directory to write the generated `*.1` man pages into (created if needed)
    #[arg(short = 'o', long = "output", value_name = "DIR")]
    pub output: PathBuf,
}

/// Run `bb man`.
///
/// # Errors
/// Returns [`FlagError`] if the output directory can't be created or a page
/// can't be written.
pub fn run(ctx: &Context, args: ManArgs) -> anyhow::Result<()> {
    std::fs::create_dir_all(&args.output).map_err(|e| {
        FlagError::new(format!(
            "could not create output directory {}: {e}",
            args.output.display()
        ))
    })?;

    let pages = render_pages();
    let count = pages.len();
    for (name, roff) in pages {
        let path = args.output.join(&name);
        std::fs::write(&path, roff)
            .map_err(|e| FlagError::new(format!("could not write {}: {e}", path.display())))?;
    }

    ctx.io.println(&format!(
        "Wrote {count} man pages to {}",
        args.output.display()
    ));
    Ok(())
}

/// Render every command in the `bb` tree to a `(filename, roff)` pair: the root
/// as `bb.1` and each subcommand as `bb-<path>.1` (e.g. `bb-pr-create.1`).
fn render_pages() -> Vec<(String, Vec<u8>)> {
    let root = crate::cli::Cli::command();
    let mut pages = Vec::new();
    render_into(&root, "", &mut pages);
    pages
}

fn render_into(cmd: &clap::Command, prefix: &str, pages: &mut Vec<(String, Vec<u8>)>) {
    let name = if prefix.is_empty() {
        cmd.get_name().to_owned()
    } else {
        format!("{prefix}-{}", cmd.get_name())
    };

    let mut roff = Vec::new();
    // Rendering to an in-memory buffer is infallible in practice; surface any
    // error as an empty page rather than panicking in a generator.
    let _ = clap_mangen::Man::new(cmd.clone()).render(&mut roff);
    pages.push((format!("{name}.1"), roff));

    for sub in cmd.get_subcommands() {
        render_into(sub, &name, pages);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bb_api::testing::FakeTransport;
    use bb_config::FileConfig;
    use bb_core::{ConfigProvider, GitClient, Transport};
    use bb_git::{ShellGit, StubRunner};

    use super::*;
    use crate::testsupport::{test_context, ScriptedPrompter};

    #[test]
    fn render_pages_covers_root_and_nested_commands() {
        let pages = render_pages();
        let names: Vec<&str> = pages.iter().map(|(n, _)| n.as_str()).collect();

        for expected in ["bb.1", "bb-pr.1", "bb-pr-create.1", "bb-completion.1"] {
            assert!(
                names.contains(&expected),
                "missing {expected}; got {names:?}"
            );
        }
        // Every page is a non-empty roff document (`.TH` title header).
        for (name, roff) in &pages {
            assert!(!roff.is_empty(), "{name} is empty");
            let text = String::from_utf8_lossy(roff);
            assert!(text.contains(".TH"), "{name} missing .TH header");
        }
    }

    #[test]
    fn run_writes_files_and_reports_count() {
        let transport: Arc<dyn Transport> = Arc::new(FakeTransport::new());
        let git: Arc<dyn GitClient> = Arc::new(ShellGit::new(Arc::new(StubRunner::new())));
        let config: Arc<dyn ConfigProvider> = Arc::new(FileConfig::blank());
        let prompter = Arc::new(ScriptedPrompter::new());
        let (ctx, bufs) = test_context(transport, git, config, prompter, false);

        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("man");
        run(
            &ctx,
            ManArgs {
                output: out.clone(),
            },
        )
        .unwrap();

        assert!(out.join("bb.1").is_file(), "bb.1 should exist");
        assert!(out.join("bb-pr-create.1").is_file(), "nested page exists");
        let n = std::fs::read_dir(&out).unwrap().count();
        assert!(
            bufs.stdout_string()
                .contains(&format!("Wrote {n} man pages")),
            "summary should report the count"
        );
    }
}
