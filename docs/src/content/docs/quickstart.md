---
title: Quickstart
description: Diagnose a Rust repository, choose a profile, and seed a warm worktree.
order: 1
category: Start
summary: Go from install to a worktree hook in a few commands.
---

## Install

```bash
cargo install cargo-warm --locked
```

`cargo-warm` is a Cargo subcommand, so either spelling works:

```bash
cargo warm --help
cargo-warm --help
```

## 1. Ask the doctor

Run this in the repository you want to optimize:

```bash
cargo warm doctor
```

You do not need a second worktree yet. In project-only mode, the doctor inspects the selected Cargo packages, Rust source size, build scripts, toolchain capabilities, and current cache shape. It recommends one of the built-in profiles and prints a starter `.agents/.cargo-warm.toml`.

If another compatible worktree already exists, the doctor also compares the two checkouts and reports freshness/path issues that can affect cache reuse.

## 2. Save the project config

For example:

```toml
version = 1
default-profile = "balanced"
clone-pressure = "auto"
manifests = ["Cargo.toml"]
unstable-bootstrap = true
```

The bootstrap line is needed only when a priming profile uses relocatable rustc state on stable/beta Rust. Nightly/dev does not need it.

For a repository with several independent Cargo workspaces:

```toml
version = 1
default-profile = "balanced"
clone-pressure = "auto"
manifests = [
  "app/Cargo.toml",
  "tools/Cargo.toml",
]
```

## 3. Warm the source checkout

For `balanced` or `deep`, use the same relocatable check family that worktrees will use:

```bash
cargo warm check
```

For `quick`, ordinary Cargo state is enough:

```bash
cargo check
```

The source checkout can keep evolving normally. Cargo-warm looks for compatible nearby worktrees with real warm state instead of assuming one fixed branch is always best.

## 4. Create and seed a worktree

```bash
git worktree add ../feature -b feature
cd ../feature
cargo warm seed
```

The destination receives a separate writable cache. On supported filesystems, the initial data is cloned copy-on-write/reflink rather than physically copied in full.

## 5. Work normally

If the project uses `balanced` or `deep`, keep using:

```bash
cargo warm check
```

Arguments after cargo-warm's options are forwarded to Cargo:

```bash
cargo warm check --workspace
cargo warm check --manifest-path crates/app/Cargo.toml
```

For `quick`, use your normal Cargo commands.

## Next steps

- Read [Profiles and configuration](/docs/profiles-and-config) to tune startup behavior.
- Read [Doctor and benchmarking](/docs/doctor-and-benchmarking) before choosing a more expensive profile.
- Put the final command in your [worktree hook](/docs/worktree-hooks).
