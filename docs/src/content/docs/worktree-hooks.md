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

Cargo-warm discovers active worktrees automatically and selects the nearest compatible, quiescent checkout that actually has warm state. `main` is only a tie-breaker; a nearby sibling agent branch can be a better seed.

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

## Relocatable compiler loop

For Rust 1.98+ projects that opt into cargo-warm's relocatable incremental artifact family, use the same compiler command in the warm source checkout and new agent worktrees:

```bash
# Source checkout, periodically or after integrating work.
cargo warm check --unstable-bootstrap --manifest-path Cargo.toml

# Worktree creation hook.
cargo warm seed --prime --unstable-bootstrap --manifest-path Cargo.toml

# Agent edit loop in the new worktree.
cargo warm check --unstable-bootstrap --manifest-path Cargo.toml
```

On nightly/dev Rust the explicit bootstrap flag is unnecessary. Stable/beta requires the opt-in because the compiler relocation switch is still unstable.

`--prime` is the strongest agent-startup mode. It moves the first destination-specific Cargo build-script boundary and rustc validation into provisioning while source bytes are still unchanged. The selected direct package's own build script may run, but path-dependency build scripts are left untouched. The worktree takes longer to create, but the agent's first real edit starts from destination-native incremental state. Omit `--prime` when startup latency matters more, when a custom build is too expensive to repeat during provisioning, or when the project already gets near-warm first-edit behavior from plain seeding.

## Active compilers

Seeding refuses a source or destination workspace with an active Cargo/rustc process. This avoids cloning a cache while it is being mutated.
