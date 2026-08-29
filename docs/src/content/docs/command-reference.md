---
title: Command reference
description: Compact reference for cargo-warm commands and script-facing options.
order: 8
category: Reference
summary: doctor, seed, check, path, status, and gc.
---

## `cargo warm doctor`

```bash
cargo warm doctor
cargo warm doctor --profile deep
cargo warm doctor --from /warm/source --to /new/worktree
cargo warm doctor --probe
cargo warm doctor --json
```

Default mode is read-only with respect to build state. With no warm peer it performs project-only diagnosis. With a peer it also compares worktree freshness/path state.

`--probe` runs `cargo check` with Cargo fingerprint logging and can compile code.

## `cargo warm seed`

```bash
cargo warm seed
cargo warm seed --profile balanced
cargo warm seed --from /warm/source --to /new/worktree
cargo warm seed --prime-mode rustc
cargo warm seed --seed-path native/cache
cargo warm seed --include-target
cargo warm seed --no-freshness-rebase
cargo warm seed --copy-fallback
```

Important options:

- `--profile <name>` selects a built-in or project-local profile.
- `--config <path>` uses an explicit config instead of auto-discovery.
- `--manifest-path <path>` overrides configured manifests; repeatable.
- `--prime-mode none|rustc|package` overrides the profile's prime.
- `--include-target` also forks target/final-output state when Cargo has a separate intermediate build directory.
- `--seed-path <path>` adds a workspace-relative portable state path; repeatable.
- `--no-freshness-rebase` disables safe mtime/build-script freshness synchronization.
- `--copy-fallback` explicitly permits a physical copy when COW/reflink is unavailable.
- `--unstable-bootstrap` explicitly allows relocatable priming on stable/beta.

Legacy `--prime` remains equivalent to package priming for compatibility.

## `cargo warm check`

```bash
cargo warm check
cargo warm check --profile balanced
cargo warm check --workspace
cargo warm check --manifest-path crates/app/Cargo.toml
```

Runs `cargo check` with cargo-warm's workspace-only rustc wrapper. Cargo flags not consumed by cargo-warm are forwarded to `cargo check`.

Project config can provide the stable/beta bootstrap opt-in, so a configured repository can keep the command itself short.

Requires Rust 1.98+ for relocatable incremental state.

## `cargo warm path`

```bash
cargo warm path
cargo warm path --workspace /repo
cargo warm path --config path/to/cargo-warm.toml
cargo warm path --manifest-path Cargo.toml --json
```

Shows Cargo's resolved workspace/build/target paths. With no manifest flags it uses the manifest set from `.agents/.cargo-warm.toml`, falling back to `Cargo.toml` when the project has no config.

## `cargo warm status`

```bash
cargo warm status
```

Lists cache roots recorded as cargo-warm-owned.

## `cargo warm gc`

```bash
cargo warm gc --dry-run
cargo warm gc
```

Removes cargo-warm-owned cache roots whose destination worktrees no longer exist.
