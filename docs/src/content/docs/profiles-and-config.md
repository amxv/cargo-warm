---
title: Profiles and configuration
description: Choose how much work cargo-warm moves into worktree provisioning.
order: 2
category: Start
summary: Built-in quick, balanced, and deep profiles plus project-local overrides.
---

Profiles answer one question: **how much setup work should happen before the developer or agent starts editing?**

They do not change cargo-warm's safety boundary. Every profile still uses private destination caches and lets Cargo/rustc validate the result.

## Built-in profiles

| Profile | Prime mode | Trade-off |
| --- | --- | --- |
| `quick` | none | Lowest provisioning latency. Forks cache state and performs safe freshness synchronization. |
| `balanced` | rustc | Adds one relocatable rustc session so large local crates can open inherited incremental state before the first edit. |
| `deep` | package | Also re-establishes the selected package's own build-script fingerprint boundary. More setup work, but useful when very large packages otherwise keep a first-edit penalty. |

Select one for a single seed:

```bash
cargo warm seed --profile quick
cargo warm seed --profile balanced
cargo warm seed --profile deep
```

`cargo warm doctor` recommends a starting profile and tells you which alternatives are worth benchmarking.

## Project config

Cargo-warm automatically reads `.agents/.cargo-warm.toml` from the Git root. Create the directory once if your repository does not already have it:

```bash
mkdir -p .agents
```

## Clone pressure is a separate axis

Profiles control **how much compiler validation** happens during provisioning. Clone pressure controls **how aggressively independent cache roots are copied at the same time**. They are intentionally orthogonal.

```toml
version = 1
default-profile = "balanced"
clone-pressure = "auto"
manifests = ["Cargo.toml"]
```

| Clone pressure | Root concurrency |
| --- | --- |
| `auto` | Adapts to available CPUs and the number of seedable cache roots. This is the default. |
| `gentle` | One cache root at a time. Lowest I/O pressure. |
| `fast` | Up to four independent cache roots at once. |
| `max` | Up to 2× logical CPUs, capped at 16 and by the number of roots. |

For deterministic tuning, bypass the preset calculation:

```toml
clone-workers = 2
```

An explicit worker count overrides clone pressure. `cargo warm doctor` shows how many warm roots exist and how the selected setting resolves on the current machine.

This knob does not change `quick`, `balanced`, or `deep`, and it does not weaken the cache safety model.

## Give Cargo a clean build-cache boundary

Cargo-warm works with Cargo's default target directory, but Cargo 1.91+ can place intermediate compiler state in a separate `build-dir`. For worktree-heavy projects, a useful project or global Cargo configuration is:

```toml
[build]
build-dir = "{cargo-cache-home}/build/{workspace-path-hash}"
```

This keeps final artifacts and intermediate cache state separate while still giving each worktree a distinct writable build directory. `cargo warm doctor` reports when a project is still mixing both roles in one directory.

```toml
version = 1
default-profile = "balanced"
clone-pressure = "auto"
manifests = ["Cargo.toml"]
```

A monorepo can list several Cargo workspaces once instead of repeating flags in every hook:

```toml
version = 1
default-profile = "balanced"
clone-pressure = "auto"
manifests = [
  "desktop/Cargo.toml",
  "services/worker/Cargo.toml",
]
```

### Stable/beta relocatable checks

The rustc relocation switch used by `balanced` and `deep` is still unstable. On stable/beta, opt in explicitly:

```toml
unstable-bootstrap = true
```

The same setting is used by both `cargo warm seed` and `cargo warm check`, so scripts do not need to repeat `--unstable-bootstrap` everywhere.

## Custom profiles

Project-specific profiles can inherit any built-in or another custom profile:

```toml
version = 1
default-profile = "agent"

[profiles.agent]
inherits = "deep"
seed-paths = ["native/cache"]
```

Available fields:

```toml
[profiles.example]
inherits = "balanced"
include-target = false
copy-fallback = false
freshness-rebase = true
prime = "rustc"            # none | rustc | package
unstable-bootstrap = true
seed-paths = ["path/to/portable/state"]
```

`seed-paths` is for unusual workspace-relative generated state that cargo-warm cannot infer safely. Prefer automatic native-artifact discovery when it works; explicit paths are an escape hatch, not a requirement for ordinary projects.

## CLI overrides

Command-line values override project config for that invocation:

```bash
cargo warm seed --profile quick
cargo warm seed --clone-pressure fast
cargo warm seed --clone-workers 2
cargo warm seed --prime-mode package
cargo warm seed --include-target
cargo warm seed --no-freshness-rebase
```

The older `--prime` flag remains a compatibility shortcut for package priming. New scripts should prefer a named profile or `--prime-mode` because the intent is clearer.

## Which profile should I commit?

Do not pick the most expensive profile by default.

1. Start with the doctor recommendation.
2. Benchmark worktree creation separately from the first representative edit.
3. Compare only the profiles the doctor suggests.
4. Commit the profile that gives the best end-to-end developer/agent loop for the repository.

Tune clone pressure independently. Use `cargo warm seed --timings` on fresh worktrees to compare the clone phase without conflating it with first-edit compiler latency.

See [Doctor and benchmarking](/docs/doctor-and-benchmarking) for a repeatable method.
