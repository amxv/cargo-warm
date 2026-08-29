---
title: Quickstart
description: Seed a new Rust worktree from an already-warm checkout.
order: 1
category: Start
summary: Put cargo-warm directly in a worktree creation hook.
---

## Install

Install from crates.io:

```bash
cargo install cargo-warm --locked
```

Both invocation forms are supported:

```bash
cargo warm --help
cargo-warm --help
```

## Warm the source checkout

Use Cargo normally in the checkout you want new worktrees to inherit from:

```bash
cargo check
```

`cargo-warm` does not own that build. It reuses the state Cargo already created.

## Create and seed a worktree

If the repository has a separate `main` worktree, the short form is enough:

```bash
git worktree add ../feature -b feature
cd ../feature
cargo warm seed
```

The new checkout receives its own private writable build cache. On macOS the seed uses an APFS clone. On supported Linux filesystems it uses a reflink.

## Use an explicit source

Worktree managers and agent runtimes usually know both paths already:

```bash
cargo warm seed \
  --from /repo/main \
  --to /repo/worktrees/feature
```

That explicit form is the stable integration primitive.

## Multiple Cargo workspaces

Repeat `--manifest-path` when one repository contains multiple independent Cargo workspaces:

```bash
cargo warm seed \
  --manifest-path src-tauri/Cargo.toml \
  --manifest-path src-tauri/sidecars/server/Cargo.toml
```
