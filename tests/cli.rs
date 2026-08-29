use std::{fs, path::PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_cargo-warm"))
}

fn isolated_cache(name: &str) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("cargo-warm-cli-test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn shows_help() {
    cli()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage: cargo-warm"))
        .stdout(predicate::str::contains("seed"))
        .stdout(predicate::str::contains("path"))
        .stdout(predicate::str::contains("status"))
        .stdout(predicate::str::contains("gc"));
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
fn cargo_subcommand_invocation_is_supported() {
    let cache = isolated_cache("subcommand");
    cli()
        .args(["warm", "status"])
        .env("XDG_CACHE_HOME", &cache)
        .assert()
        .success()
        .stdout("cargo-warm: no recorded seeded caches\n");
    let _ = fs::remove_dir_all(cache);
}

#[test]
fn direct_status_invocation_is_supported() {
    let cache = isolated_cache("direct");
    cli()
        .arg("status")
        .env("XDG_CACHE_HOME", &cache)
        .assert()
        .success()
        .stdout("cargo-warm: no recorded seeded caches\n");
    let _ = fs::remove_dir_all(cache);
}
