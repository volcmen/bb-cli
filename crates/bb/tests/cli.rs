//! Black-box smoke tests for the `bb` binary (exit codes + version/help).

use assert_cmd::Command;
use predicates::prelude::*;

fn bb() -> Command {
    Command::cargo_bin("bb").expect("bb binary builds")
}

#[test]
fn version_flag_prints_version() {
    let version = env!("CARGO_PKG_VERSION");
    bb().arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("bb").and(predicate::str::contains(version)));
}

#[test]
fn version_subcommand_prints_full_version() {
    bb().arg("version")
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "bb version {}",
            env!("CARGO_PKG_VERSION")
        )));
}

#[test]
fn unknown_command_exits_2() {
    bb().arg("definitely-not-a-real-command")
        .assert()
        .failure()
        .code(2);
}

#[test]
fn no_args_prints_help() {
    bb().assert()
        .success()
        .stdout(predicate::str::contains("Bitbucket"));
}
