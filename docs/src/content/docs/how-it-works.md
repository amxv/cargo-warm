---
title: How it works
description: The mental model behind private warm Cargo state.
order: 5
category: Concepts
summary: Fork build state, repair only proven freshness, then let Cargo and rustc validate normally.
---

## The problem

A new Git worktree can start at the same revision as a warm checkout but still have cold Cargo state. Rebuilding the same dependency graph for every temporary checkout is wasteful.

Sharing one writable target/build directory is tempting, but concurrent worktrees then share locks, invalidations, generated paths, and mutation.

Cargo-warm takes a different approach.

## Fork, do not share

```text
warm checkout cache
        │
        │ copy-on-write clone / reflink
        ▼
new worktree private cache
        │
        └─ Cargo + rustc validate normally
```

The destination owns a distinct path and can mutate it independently. On supported filesystems, unchanged blocks remain physically shared until one copy changes.

Cargo-warm asks Cargo for the resolved cache paths instead of reproducing Cargo's workspace hashing rules itself.

## Freshness is repaired conservatively

Git creates a new checkout with new filesystem timestamps even when the committed bytes are identical. Cargo can therefore see an exact worktree as newer than copied fingerprints.

Cargo-warm can synchronize that metadata, but only inside a narrow proof boundary:

1. source and destination are worktrees of the same repository;
2. the tracked path is clean in both;
3. both Git index entries refer to the same object;
4. immediately before changing destination mtimes, the bytes are compared again.

If any proof fails, Cargo gets the ordinary stale input and rebuilds it.

## Build scripts need extra care

Build scripts can cache absolute checkout paths or watch generated/native files outside Cargo's main intermediate directory.

Cargo-warm reads Cargo's cached build-script directives and handles only mappings it understands safely. For common native link outputs it can materialize just the final library/artifact and any required symlink instead of cloning an opaque Swift/Clang compiler cache.

Unknown path-bearing state remains stale. Normal Cargo revalidation is safer than silently linking back into another worktree.

## Relocatable rustc state

For Rust 1.98+, `cargo warm check` uses a workspace-only rustc wrapper so local crates use one virtual working-directory identity for incremental compilation.

Third-party dependencies are not routed through the wrapper. Cargo still invokes rustc in the destination, and path-sensitive compiler inputs are recomputed there.

The compiler switch is currently unstable. Nightly/dev can accept it directly. Stable/beta requires explicit bootstrap opt-in, which cargo-warm scopes to workspace rustc invocations and never enables silently.

## Priming profiles

A copied cache can be structurally warm while the first real edit still pays a one-time destination validation cost. Profiles let the project decide where to pay that cost.

- `quick` does not prime the compiler.
- `balanced` temporarily advances the selected package's root Rust target timestamp, runs the relocatable check, then restores the exact timestamp.
- `deep` also advances that direct package's own `build.rs` timestamp so Cargo re-establishes the package build boundary during provisioning.

No source bytes are changed. Path-dependency build scripts are not selected by the deep prime.

See [Architecture](/docs/architecture) for the implementation boundaries and [Doctor and benchmarking](/docs/doctor-and-benchmarking) for deciding whether the extra setup cost is worthwhile.
