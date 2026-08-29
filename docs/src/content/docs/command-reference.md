---
title: Command reference
description: The current cargo-warm command, diagnostics, and integration surface.
order: 5
category: Reference
summary: path, seed, doctor, experimental check, status, gc, and the flags intended for scripts.
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
cargo warm seed --prime --unstable-bootstrap
cargo warm seed --seed-path native/.build/output.a
cargo warm seed --no-freshness-rebase
cargo warm seed --copy-fallback
```

`--include-target` clones final/link target outputs in addition to the modern Cargo `build_directory`.

Without `--from`, cargo-warm ranks active worktrees by Git graph distance. Exact revisions and nearby sibling branches can both seed a new worktree; compiling or incompatible candidates are skipped, and candidates with real warm cache roots are preferred.

For ignored native state referenced by `rustc-link-search` / `rustc-link-lib`, cargo-warm automatically forks only the final linkable artifact(s) and any required workspace-relative symlink. It does not copy an entire Swift/Clang compiler cache merely because a build script links through that directory. `--seed-path` is a repeatable escape hatch for unusual portable state cargo-warm cannot infer.

By default, `seed` also rebases false freshness misses only when it can prove the destination is equivalent: identical clean Git blobs, equivalent watched build-script trees, and safely relocatable build-script path directives. `--no-freshness-rebase` disables that behavior.

`--prime` forces one no-content-change relocatable compiler session after the fork. Cargo-warm temporarily advances the root target source mtime, runs `cargo warm check`, and restores the exact original timestamps. This converts inherited compiler state into destination-native incremental state before an agent edits anything. Stable/beta requires `--unstable-bootstrap`; nightly/dev does not.

`--copy-fallback` explicitly allows a normal physical copy if a COW/reflink clone is unavailable. It is intentionally opt-in.

## `cargo warm doctor`

```bash
cargo warm doctor
cargo warm doctor --from /warm/main --to /new/worktree
cargo warm doctor --manifest-path Cargo.toml --json
```

The default mode is read-only with respect to Cargo build state. It reports:

- whether source and destination are at the same Git revision;
- whether both worktrees are clean;
- tracked-file mtime skew for an exact clean revision;
- source and destination build-cache presence;
- local workspace packages with build scripts.

To ask Cargo why the destination actually rebuilds:

```bash
cargo warm doctor --probe
```

Probe mode runs `cargo check` and captures Cargo's fingerprint diagnostics. It can compile code, so use it when you want measured rebuild reasons rather than a cheap preflight.

## `cargo warm check` (experimental)

```bash
cargo warm check
cargo warm check --unstable-bootstrap --workspace
cargo warm check --unstable-bootstrap --manifest-path crates/app/Cargo.toml
```

This creates a separate workspace-local artifact family through Cargo's workspace rustc-wrapper mechanism. Local crates are compiled with rustc's relocatable working-directory mode so a seeded worktree can load incremental state produced in another checkout.

The source checkout must also be warmed with `cargo warm check`; an ordinary `cargo check` does not produce the same workspace-wrapper artifact family.

Requirements:

- Rust 1.98 or newer;
- nightly/dev toolchain for the compiler flag without environment changes; or
- `--unstable-bootstrap` as an explicit stable/beta experiment.

Stable bootstrap mode scopes `RUSTC_BOOTSTRAP` to the current workspace crate and forbids unstable source features. Rust code can still observe that environment variable through `env!` / `option_env!`, so cargo-warm never enables it implicitly.

Only `cargo check` is wrapped in this first 3B slice. Build, test, and Clippy modes remain ordinary Cargo behavior until their relocation semantics and developer UX have been benchmarked separately.

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
