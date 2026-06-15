//! Black-box smoke tests for the `bb` binary (exit codes + version/help).

use assert_cmd::Command;
use predicates::prelude::*;

fn bb() -> Command {
    Command::cargo_bin("bb").expect("bb binary builds")
}

#[test]
fn version_flag_prints_version() {
    bb().arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("bb").and(predicate::str::contains("0.1.0")));
}

#[test]
fn version_subcommand_prints_full_version() {
    bb().arg("version")
        .assert()
        .success()
        .stdout(predicate::str::contains("bb version 0.1.0"));
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
