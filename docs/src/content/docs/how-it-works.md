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

## What can still miss

Even an exact-base worktree can invalidate state because of:

- source or build-script mtimes;
- build-script outputs that contain checkout-local absolute paths;
- changed environment/configuration;
- feature or target differences;
- changed source;
- rustc incremental query invalidation.

The next stage of the project is to diagnose these misses precisely and make nearby compiler state more portable and selectable without weakening isolation.
