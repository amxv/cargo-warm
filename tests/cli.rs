use assert_cmd::Command;
use predicates::prelude::*;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_cargo-warm"))
}

#[test]
fn shows_help() {
    cli()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"))
        .stdout(predicate::str::contains("hello"));
}

#[test]
fn shows_version() {
    cli()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn greets_default_name() {
    cli()
        .arg("hello")
        .assert()
        .success()
        .stdout("Hello, world!\n");
}

#[test]
fn greets_a_name() {
    cli()
        .args(["hello", "agent"])
        .assert()
        .success()
        .stdout("Hello, agent!\n");
}
