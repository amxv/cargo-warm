---
title: Cache lifecycle
description: Inspect and clean the private cache roots cargo-warm owns.
order: 7
category: Operate
summary: See what cargo-warm created and remove orphaned worktree caches conservatively.
---

## Status

```bash
cargo warm status
```

The registry records cache roots cargo-warm created, the worktree they belong to, and whether that worktree/cache is still available.

Cargo-warm does not claim ownership of arbitrary Cargo caches it merely observed.

## Garbage collection

Preview cleanup first:

```bash
cargo warm gc --dry-run
```

Then remove cargo-warm-owned cache roots whose destination worktrees no longer exist:

```bash
cargo warm gc
```

GC is deliberately conservative. It does not delete an active worktree's cache and does not scan the filesystem looking for unrelated Cargo directories to remove.

## Cache pressure

A cache can become too large to be useful. Very large stale `deps/` populations can add filesystem and Cargo bookkeeping overhead even when the actual compiler work is incremental.

`cargo warm doctor` reports simple cache-shape counts so unusually bloated warm sources are visible. If a source has accumulated many obsolete build families, benchmark after pruning stale state as well as after changing cargo-warm profiles.

The goal is not maximal cache retention. It is the smallest warm state that gives the best edit loop.
