---
title: Worktree hooks
description: Integrate cargo-warm with IDEs, coding agents, and custom worktree managers.
order: 4
category: Integrate
summary: Keep the hook tiny; put project behavior in .agents/.cargo-warm.toml.
---

## Recommended hook

Commit `.agents/.cargo-warm.toml`, then run one command after Git creates the worktree:

```bash
cargo warm seed
```

That is the preferred integration surface. Manifests, profile choice, bootstrap opt-in, and unusual seed paths belong in project config rather than being duplicated across every editor/agent integration.

## Deterministic source paths

If the orchestrator already knows the warm and destination paths:

```bash
cargo warm seed \
  --from "$WARM_CHECKOUT" \
  --to "$NEW_WORKTREE"
```

Without `--from`, cargo-warm ranks Git worktrees by graph distance, skips active/incompatible candidates, and prefers candidates that actually have warm cache roots. A nearby sibling branch can be a better source than `main`.

## Agent compiler loop

For a project using `balanced` or `deep`, keep the source and agent worktrees in the same relocatable artifact family:

```bash
# Warm source checkout after integrating work.
cargo warm check

# New agent worktree.
cargo warm seed

# Agent edit loop.
cargo warm check
```

With `unstable-bootstrap = true` in `.agents/.cargo-warm.toml`, stable/beta projects do not need to repeat the flag in every command.

## Best-effort provisioning

Cargo-warm is an optimization. An orchestrator can decide that failure should fall back to an ordinary cold cache:

```bash
if command -v cargo-warm >/dev/null 2>&1; then
  cargo warm seed || echo "cargo-warm unavailable; continuing cold" >&2
fi
```

Cargo-warm itself fails closed when it cannot prove a requested cache operation is safe. The decision to block or continue belongs to the worktree manager.

## Project build controls

`deep` can intentionally run the selected package's own build script during provisioning. The prime inherits the hook's environment, so project-supported build-script controls can be set around `cargo warm seed` just as they would be around `cargo check`.

Path-dependency build scripts are deliberately not selected by the deep prime.

## Concurrency

Seeding refuses a source or destination with an active Cargo/rustc process. A worktree manager should schedule the seed before starting the agent's compiler work.

Different destination worktrees still receive different writable caches, so agents do not contend on one shared Cargo build directory after provisioning.
