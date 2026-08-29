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
        .stdout(predicate::str::contains("doctor"))
        .stdout(predicate::str::contains("check"))
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

#[test]
fn stable_check_requires_explicit_bootstrap_opt_in() {
    let root = isolated_cache("stable-check-no-config");
    cli()
        .arg("check")
        .current_dir(&root)
        .assert()
        .failure()
        .stderr(predicate::str::contains("--unstable-bootstrap"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn check_refuses_to_replace_an_existing_workspace_wrapper() {
    cli()
        .args(["check", "--unstable-bootstrap"])
        .env("RUSTC_WORKSPACE_WRAPPER", "/tmp/existing-wrapper")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "RUSTC_WORKSPACE_WRAPPER is already set",
        ));
}

#[test]
fn project_config_can_opt_stable_check_into_bootstrap() {
    let root = isolated_cache("configured-check");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join(".agents")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='configured-check'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "pub fn configured() -> bool { true }\n",
    )
    .unwrap();
    fs::write(
        root.join(".agents/.cargo-warm.toml"),
        "version = 1\ndefault-profile = 'balanced'\nunstable-bootstrap = true\n",
    )
    .unwrap();

    cli()
        .arg("check")
        .current_dir(&root)
        .env("CARGO_TARGET_DIR", root.join("target"))
        .assert()
        .success();

    let _ = fs::remove_dir_all(root);
}

#[test]
fn doctor_probe_uses_the_configured_relocatable_check_family() {
    let root = isolated_cache("doctor-probe-configured");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join(".agents")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='doctor-probe-configured'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn value() -> u8 { 1 }\n").unwrap();
    fs::write(
        root.join(".agents/.cargo-warm.toml"),
        "version = 1\ndefault-profile = 'balanced'\nunstable-bootstrap = true\n",
    )
    .unwrap();

    cli()
        .args(["doctor", "--probe", "--json"])
        .current_dir(&root)
        .env("CARGO_TARGET_DIR", root.join("target"))
        .assert()
        .success()
        .stdout(predicate::str::contains("\"name\": \"balanced\""))
        .stdout(predicate::str::contains("\"cargo_success\": true"));

    let _ = fs::remove_dir_all(root);
}
