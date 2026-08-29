---
title: Changelog
description: User-visible release history for cargo-warm.
order: 99
category: Reference
summary: New commands, compatibility changes, and cache behavior by release.
---

## 0.3.0 - 2026-08-29

- Add built-in `quick`, `balanced`, and `deep` startup profiles plus project-defined inherited profiles.
- Add project configuration at `.agents/.cargo-warm.toml`; configured manifests and stable-toolchain bootstrap settings are shared by `seed`, `doctor`, `path`, and `check`.
- Expand `cargo warm doctor` into a project advisor: it now works before a second worktree exists, recommends profiles, reports cache pressure and Cargo build-directory shape, and prints a starter project config.
- Make `doctor --probe` use the same ordinary or relocatable compiler family as the selected profile instead of probing a different build mode.
- Avoid repeated manifest/source preflight work during automatic source selection, and keep the tracked-file byte proof on the mutating freshness path instead of reading every eligible file twice.
- Pass rustc version/capability probes through the workspace wrapper unchanged; relocatable settings now apply only to actual crate compilations.
- Rewrite the public README/docs around setup, profiles, diagnostics, hooks, and architecture, and replace the landing page with a workflow-focused product overview.

## 0.2.1 - 2026-08-29

- Strengthen opt-in priming so it can re-establish the selected direct package's build-script fingerprint boundary before the first edit.
- Keep path-dependency build scripts out of that stronger prime so unrelated native toolchains are not awakened merely because they are dependencies.
- Preserve exact source timestamps around priming and keep a successfully forked private cache usable even when the optional prime fails.

## 0.2.0 - 2026-08-29

- Add safe freshness synchronization for clean equivalent worktree inputs.
- Add relocatable incremental `cargo warm check` support for Rust 1.98+.
- Add opt-in provisioning prime support.
- Rank nearby compatible worktrees automatically instead of assuming one fixed source branch.
- Discover common path-bearing native link outputs and fork final link artifacts without copying opaque compiler caches.
- Add `cargo warm doctor` diagnostics and Cargo fingerprint probes.
- Preserve repositories that intentionally do not commit `Cargo.lock`.

## 0.1.0 - 2026-08-29

- Fork warm Cargo build state into a destination worktree's private cache.
- Use APFS clone-on-write on macOS and filesystem reflinks on Linux.
- Support automatic or explicit warm-source worktrees.
- Support several Cargo workspaces through repeated manifests.
- Refuse shared mutable cache paths, incompatible toolchains, and active compiler state.
- Track cargo-warm-owned cache roots for conservative cleanup.
