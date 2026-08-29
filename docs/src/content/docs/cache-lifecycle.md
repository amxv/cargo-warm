---
title: Cache lifecycle
description: Inspect and clean cache roots created by cargo-warm.
order: 4
category: Operations
summary: Track owned cache roots and delete orphaned worktree state conservatively.
---

## Status

```bash
cargo warm status
```

Each recorded cache is classified as available, orphaned, or missing based on its destination worktree and cache path.

## Garbage collection

Preview first:

```bash
cargo warm gc --dry-run
```

Then remove recorded cache roots whose destination worktrees no longer exist:

```bash
cargo warm gc
```

The current GC is intentionally conservative. It does not delete arbitrary Cargo caches it did not create.

Future lifecycle work will add global/per-repository budgets, active-worktree protection, and LRU or benefit-aware retention after orphan cleanup.
