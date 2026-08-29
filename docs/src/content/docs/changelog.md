---
title: Changelog
description: User-visible release history for cargo-warm.
order: 99
category: Reference
summary: New commands, compatibility changes, and cache behavior by release.
---

Keep the newest release first. Focus on behavior developers notice, especially worktree compatibility, cache reuse, disk growth, and installation changes.

## 0.1.0 - 2026-08-29

- Fork a warm Cargo `build_directory` into a new worktree's private writable cache.
- Use APFS clone-on-write on macOS and filesystem reflinks on Linux.
- Discover a separate `main` worktree automatically or accept explicit `--from` / `--to` paths.
- Support multiple Cargo workspaces through repeated `--manifest-path` flags.
- Refuse shared mutable cache paths, incompatible toolchains, and active compiler state.
- Track seeded cache roots and garbage-collect only orphaned state owned by cargo-warm.
