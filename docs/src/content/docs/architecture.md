---
title: Architecture
description: A compact tour of cargo-warm's internal boundaries and safety invariants.
order: 6
category: Concepts
summary: Source selection, cache forking, freshness proof, native state, and relocatable compiler priming.
---

This page is intentionally a map, not a line-by-line implementation guide. The source is the authority when you need exact behavior.

## 1. Resolve the project

Cargo-warm reads `.agents/.cargo-warm.toml`, resolves the selected profile, and asks Cargo metadata for each configured manifest.

It records the actual workspace, intermediate build directory, target directory, package roots, and toolchain identity. It does not recreate Cargo's path hashing rules.

## 2. Choose a warm source

When `--from` is omitted, cargo-warm enumerates Git worktrees and ranks them by graph distance from the destination.

Candidates are rejected when:

- their Cargo/rustc toolchain differs;
- Cargo or rustc is actively mutating the checkout;
- configured manifests cannot be resolved.

Among compatible candidates, a worktree with real warm cache roots is preferred. `main` is only a tie-breaker.

## 3. Fork private state

The selected Cargo intermediate directory is cloned to the destination's resolved path.

- macOS first uses the native APFS `clonefile()` path through a safe Rust reflink wrapper. The complete directory tree is cloned copy-on-write in one filesystem operation; file metadata used by Cargo remains intact. If that path is unavailable for a tree, cargo-warm falls back to its metadata-preserving parallel APFS clone implementation.
- Linux uses filesystem reflinks when supported.
- a physical copy is never an accidental fallback; it requires explicit opt-in.

Independent cache roots can be cloned concurrently according to the project-level clone-pressure setting. This is independent of the compiler startup profile.

Each clone is written to a temporary path and renamed into place so a partial seed is not published as a completed cache. If any concurrent clone fails, successful siblings from that batch are rolled back.

While the filesystem is cloning, cargo-warm scans the warm source's cached build-script metadata in parallel. This hides read-only planning work behind the copy-on-write operation instead of serializing every seed phase.

## 4. Materialize portable native outputs

Build-script output is scanned for path-bearing linker directives. When cargo-warm can map a missing ignored search path to concrete final link libraries, it forks those final artifacts (and required relative symlinks) into the destination.

It deliberately does not copy an entire native compiler cache merely because a build script happened to link through it.

## 5. Synchronize safe freshness

For eligible clean tracked files, Git object identity is used as a cheap filter. The mutating pass then verifies bytes immediately before mirroring source mtimes to the destination.

Build-script watched paths and cached path directives have their own equivalence checks. A blocker stops freshness synchronization for the unsafe boundary instead of trusting cross-worktree state.

## 6. Prime when requested

`balanced` and `deep` run `cargo warm check` after the fork.

The prime never edits source bytes. It temporarily adjusts timestamps on a small set of direct-package triggers so Cargo/rustc open and validate the inherited state in the destination, then restores the original timestamps.

`deep` includes the direct package's `custom-build` target. Path dependencies are not included, which avoids waking unrelated native build systems merely because they are dependencies of the selected package.

## 7. Track ownership

Cargo-warm records cache roots it created. `status` and `gc` operate only on that registry, which lets cleanup remain conservative.

## Core invariants

The design is built around a few rules:

- never share mutable destination state between worktrees;
- never copy an actively mutating compiler cache;
- never silently cross toolchain identities;
- never use timestamp repair to hide a byte mismatch;
- never reinterpret unknown path-bearing build-script state as safe;
- never make physical multi-gigabyte copying an implicit fallback;
- always let Cargo/rustc be the final correctness authority.
