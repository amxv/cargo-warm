---
title: Changelog
description: User-visible release history for cargo-warm.
order: 99
category: Reference
summary: New commands, compatibility changes, and cache behavior by release.
---

Keep the newest release first. Focus on behavior developers notice, especially worktree compatibility, cache reuse, disk growth, and installation changes.

## 0.2.0 - 2026-08-29

- Make exact-revision worktrees Cargo-fresh immediately by rebasing tracked source and build-script freshness only after proving destination bytes are equivalent.
- Add relocatable rustc incremental state through `cargo warm check`, using Rust 1.98+'s cwd-remapping compiler support so nearby worktrees can reuse the same semantic cache without sharing mutable directories.
- Add `cargo warm seed --prime` for agent provisioning: pay the one-time destination validation before the agent starts, without changing source bytes or leaving timestamps modified.
- Select the nearest compatible warm worktree automatically, including sibling agent branches, while skipping active compilers and incompatible toolchains.
- Discover path-bearing build-script native outputs and COW-fork only final linkable artifacts plus required symlinks instead of cloning opaque Swift/Clang compiler caches.
- Add `cargo warm doctor` diagnostics for source equivalence, path-sensitive build-script output, cache availability, and actual Cargo fingerprint rebuild reasons.
- Preserve library-style repositories that do not commit `Cargo.lock`; metadata and priming remove a lockfile only when cargo-warm itself caused it to appear.
- Golden Goose benchmark: an exact brand-new worktree's first no-op check fell from the old ~11m01s 3A relocation case to 2.56s versus 2.60s on warm main. With `--prime`, the first real edit checked in 33.24s versus 37.88s on warm main; steady-state worktree edits measured 39.45s.

## 0.1.0 - 2026-08-29

- Fork a warm Cargo `build_directory` into a new worktree's private writable cache.
- Use APFS clone-on-write on macOS and filesystem reflinks on Linux.
- Discover a separate `main` worktree automatically or accept explicit `--from` / `--to` paths.
- Support multiple Cargo workspaces through repeated `--manifest-path` flags.
- Refuse shared mutable cache paths, incompatible toolchains, and active compiler state.
- Track seeded cache roots and garbage-collect only orphaned state owned by cargo-warm.
