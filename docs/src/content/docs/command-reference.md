---
title: Command reference
description: The current cargo-warm command and integration surface.
order: 5
category: Reference
summary: path, seed, status, gc, and the flags intended for scripts.
---

## `cargo warm path`

```bash
cargo warm path
cargo warm path --workspace /path/to/repo
cargo warm path --manifest-path Cargo.toml --json
```

Shows Cargo's resolved workspace, build, and target paths.

## `cargo warm seed`

```bash
cargo warm seed
cargo warm seed --from /warm/main --to /new/worktree
cargo warm seed --manifest-path Cargo.toml
cargo warm seed --manifest-path app/Cargo.toml --manifest-path tools/Cargo.toml
cargo warm seed --include-target
cargo warm seed --copy-fallback
```

`--include-target` clones final/link target outputs in addition to the modern Cargo `build_directory`.

`--copy-fallback` explicitly allows a normal physical copy if a COW/reflink clone is unavailable. It is intentionally opt-in.

## `cargo warm status`

```bash
cargo warm status
```

Lists cache roots recorded by cargo-warm.

## `cargo warm gc`

```bash
cargo warm gc --dry-run
cargo warm gc
```

Deletes only recorded cache roots whose destination workspaces are gone.

## Development commands

```bash
just check-fast
just check
just build
just docs-check
just docs-build
dist plan
```
