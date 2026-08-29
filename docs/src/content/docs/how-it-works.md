---
title: How it works
description: Why cargo-warm forks build state instead of sharing one target directory.
order: 2
category: Concepts
summary: Copy-on-write inheritance with private mutable Cargo state.
---

## The problem

A new Git worktree can start from exactly the same revision as a warm checkout and still have a different Cargo build directory. The source is nearly identical, but the first check may behave like a cold build.

Sharing one writable Cargo directory across active worktrees is not the solution. It introduces build locks, contention, path-bearing state, and cross-checkout invalidation.

## The seed model

```text
warm checkout build state
          |
          | filesystem COW clone / reflink
          v
new worktree private build state
          |
          +-- Cargo and rustc validate normally
```

`cargo-warm` asks `cargo metadata` for the resolved `build_directory` and `target_directory`. It does not duplicate Cargo's workspace hashing rules.

On modern Cargo versions, `build_directory` contains the expensive intermediate compiler state and is seeded by default. `target_directory` is left alone unless `--include-target` is requested.

## Correctness boundary

The copied state is never trusted as the answer. After seeding, ordinary Cargo and rustc freshness and incremental logic runs exactly as it would for any other build directory.

This makes a seed a hint about where to start, not a replacement build system.

That distinction matters for worktree relocation. It is tempting to restore destination source mtimes to the warm checkout so Cargo considers copied local artifacts fresh. `cargo-warm` deliberately does not do that. Rust code can observe checkout-local compiler inputs such as `env!("CARGO_MANIFEST_DIR")`; bypassing rustc after relocation can therefore reuse an artifact containing the old checkout path.

## What can still miss

Even an exact-base worktree can invalidate state because of:

- source or build-script mtimes;
- build-script outputs that contain checkout-local absolute paths;
- changed environment/configuration;
- feature or target differences;
- changed source;
- rustc incremental query invalidation.

Use `cargo warm doctor` to separate these failure classes. For an exact clean worktree it identifies mtime skew and build-script boundaries without compiling. `cargo warm doctor --probe` runs `cargo check` under Cargo's fingerprint logger and classifies the actual dirty reasons.

The deeper compiler goal is not to make Cargo blindly accept relocated local artifacts. It is to let Cargo invoke rustc in the destination checkout while rustc starts from a nearby incremental state whose source identities are portable enough that unchanged queries can remain green. Path-sensitive inputs must still be recomputed normally.

## Freshness rebasing

Git worktree creation gives files and directories fresh mtimes even when their committed bytes are identical to the warm checkout. Cargo's normal freshness model can therefore classify an exact-base worktree as changed before rustc gets a chance to reuse incremental state.

After the COW fork, cargo-warm can rebase that metadata without trusting copied artifacts as answers. A tracked file is eligible only when both worktrees are in the same repository, neither copy has tracked edits, and both Git index entries point to the same blob. Build-script `rerun-if-changed` inputs are read from Cargo's own cached fingerprint data; watched directory trees must be recursively byte-equivalent before their mtimes are mirrored.

Build-script output is handled similarly. If a cached Cargo directive embeds the source checkout, cargo-warm rewrites the cloned destination output only for supported path directives and only when the corresponding destination state can be made private and equivalent. For ignored `rustc-link-search` state, it pairs the search path with the build script's `rustc-link-lib` directives and COW-forks only matching final libraries plus a required relative symlink. Opaque native compiler caches are left behind. An unknown directive remains a blocker so Cargo reruns the build script rather than reading or linking across worktree boundaries. Repeatable `--seed-path` remains an explicit fallback.

## Experimental relocatable checks

Rust 1.98 changed the unstable `remap-cwd-prefix` behavior so the physical compiler working directory no longer poisons incremental compatibility. `cargo warm check` uses that capability through Cargo's workspace-only rustc wrapper: third-party dependencies are not routed through the wrapper, while workspace crates get one stable virtual working-directory identity.

This is different from restoring source mtimes or declaring copied artifacts fresh. Cargo still sees the new worktree's files and invokes rustc. Rustc loads the inherited dependency graph, validates the destination inputs, and recomputes path-sensitive results such as `env!("CARGO_MANIFEST_DIR")` while retaining unrelated incremental state.

The underlying compiler flag is still unstable. cargo-warm therefore uses it directly only on nightly/dev toolchains. Stable/beta use requires the explicit `--unstable-bootstrap` experiment. Bootstrap is scoped to each workspace crate and unstable source features are forbidden, but the `RUSTC_BOOTSTRAP` environment variable remains observable by Rust code.

### Priming before the agent starts

An exact seeded worktree can be immediately Cargo-fresh without rustc having opened the relocated incremental database yet. On a very large crate, the first *actual source edit* can therefore pay a one-time relocation-validation pass before settling to the same steady-state latency as the warm source checkout.

`cargo warm seed --prime` moves that pass into worktree provisioning without changing source bytes. Cargo-warm selects the root Rust target source from Cargo metadata, saves its exact access/modification timestamps, advances only the modification time long enough for Cargo to invoke rustc, runs the relocatable check, and restores both timestamps. If priming fails, the timestamps are restored and the already-forked cache remains a safe 3A/3B starting point. If the process is forcibly killed before restoration, Cargo merely sees a newer source mtime and recompiles conservatively.

This is intentionally optional. A small crate may not have a meaningful first-relocation penalty; a giant monolith may prefer a longer provisioning step so a newly started coding agent experiences warm-main-like edit latency from its first compile.
