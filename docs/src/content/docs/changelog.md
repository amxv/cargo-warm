---
title: Changelog
description: User-visible release history for cargo-warm.
order: 99
category: Reference
summary: New commands, compatibility changes, and cache behavior by release.
---

Keep the newest release first. Focus on behavior developers notice, especially worktree compatibility, cache reuse, disk growth, and installation changes.

## 0.2.1 - 2026-08-29

- Strengthen `cargo warm seed --prime` so the destination re-establishes the selected package's Cargo build-script fingerprint boundary as well as its relocatable rustc state before the agent starts editing.
- Prime only the package named by each manifest (or default members for a virtual workspace), leaving path-dependency build scripts untouched so native dependency toolchains are not needlessly awakened.
- Preserve source bytes and restore exact timestamps after the stronger prime just as before; failed priming still leaves the already-forked private cache safe to use.
- Golden Goose benchmark: the released 0.2.0 hook still showed a 59.22s first-edit relocation tax versus 37.04s on warm main. Priming the direct package's root target plus its own build script reduced the new-worktree first edit to 41.47s without rerunning the Swift path dependency.

## 0.2.0 - 2026-08-29

- Make exact-revision worktrees Cargo-fresh immediately by rebasing tracked source and build-script freshness only after proving destination bytes are equivalent.
- Add relocatable rustc incremental state through `cargo warm check`, using Rust 1.98+'s cwd-remapping compiler support so nearby worktrees can reuse the same semantic cache without sharing mutable directories.
- Add `cargo warm seed --prime` for agent provisioning: pay the one-time destination validation before the agent starts, without changing source bytes or leaving timestamps modified.
- Select the nearest compatible warm worktree automatically, including sibling agent branches, while skipping active compilers and incompatible toolchains.
- Discover path-bearing build-script native outputs and COW-fork only final linkable artifacts plus required symlinks instead of cloning opaque Swift/Clang compiler caches.
- Add `cargo warm doctor` diagnostics for source equivalence, path-sensitive build-script output, cache availability, and actual Cargo fingerprint rebuild reasons.
- Preserve library-style repositories that do not commit `Cargo.lock`; metadata and priming remove a lockfile only when cargo-warm itself caused it to appear.
- Golden Goose benchmark: an exact brand-new worktree's first no-op check fell from the old ~11m01s 3A relocation case to 2.56s versus 2.60s on warm main. Follow-up release-level testing showed that the root-target-only prime could still leave a first-edit relocation tax on the largest local crate; 0.2.1 strengthens that boundary.

## 0.1.0 - 2026-08-29

- Fork a warm Cargo `build_directory` into a new worktree's private writable cache.
- Use APFS clone-on-write on macOS and filesystem reflinks on Linux.
- Discover a separate `main` worktree automatically or accept explicit `--from` / `--to` paths.
- Support multiple Cargo workspaces through repeated `--manifest-path` flags.
- Refuse shared mutable cache paths, incompatible toolchains, and active compiler state.
- Track seeded cache roots and garbage-collect only orphaned state owned by cargo-warm.
