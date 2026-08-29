---
title: Worktree hooks
description: Integrate cargo-warm with IDEs, agents, and custom worktree provisioning scripts.
order: 3
category: Operations
summary: One command after worktree creation, with explicit flags when orchestration already knows the paths.
---

## Minimal hook

Run this from the new worktree after Git creates it:

```bash
cargo warm seed
```

If `main` is checked out elsewhere in the same repository, cargo-warm discovers it automatically.

## Explicit orchestration

For deterministic automation, prefer explicit paths:

```bash
cargo warm seed \
  --from "$WARM_CHECKOUT" \
  --to "$NEW_WORKTREE"
```

A hook can add one or more `--manifest-path` flags for monorepos.

## Best-effort integration

A worktree manager may choose to treat seeding as an optimization rather than a hard dependency:

```bash
if command -v cargo-warm >/dev/null 2>&1; then
  cargo warm seed || echo "cargo-warm unavailable; continuing cold" >&2
fi
```

Whether seed failure should block worktree creation is an orchestrator policy. cargo-warm itself fails closed when it cannot prove the requested cache operation is safe.

## Active compilers

Seeding refuses a source or destination workspace with an active Cargo/rustc process. This avoids cloning a cache while it is being mutated.
