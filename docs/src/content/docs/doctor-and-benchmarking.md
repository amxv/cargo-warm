---
title: Doctor and benchmarking
description: Turn cache symptoms into a profile choice you can verify with measurements.
order: 3
category: Start
summary: Project diagnosis, worktree comparison, Cargo fingerprint probes, and a repeatable benchmark method.
---

## Project diagnosis

Run:

```bash
cargo warm doctor
```

With one checkout, the doctor runs in project-only mode. It reports:

- the active cargo-warm profile and config;
- selected Cargo packages and manifests;
- Rust toolchain/channel support;
- local Rust source size;
- direct and path-dependency build scripts;
- current Cargo cache shape;
- whether Cargo has a separate intermediate `build-dir` or still mixes build cache and final target artifacts;
- a recommended profile and alternatives worth testing.

The recommendation is deliberately a starting point, not an oracle. Static analysis cannot know whether your `build.rs` takes 20ms or two minutes, nor what kind of edit dominates your real workflow.

## Compare two worktrees

If a warm checkout and a new worktree both exist, cargo-warm automatically selects a compatible warm peer. You can also be explicit:

```bash
cargo warm doctor \
  --from /repo/main \
  --to /repo/worktrees/feature
```

The comparison adds:

- Git revision/cleanliness checks;
- tracked-file mtime skew;
- safe freshness candidates;
- checkout-local build-script paths;
- ignored native outputs cargo-warm can materialize automatically;
- source/destination cache presence.

Use JSON when another tool or agent should consume the report:

```bash
cargo warm doctor --json
```

## Ask Cargo why it rebuilds

Static diagnosis does not compile. When you need the actual dirty reasons, add `--probe`:

```bash
cargo warm doctor --probe
```

Probe mode runs `cargo check` with Cargo fingerprint logging enabled and groups common causes such as:

- changed files;
- build-script watched-path changes;
- dependency fingerprint changes;
- environment/config changes;
- compiler changes;
- filesystem freshness misses.

Because it can compile code, use `--probe` after the cheap diagnosis points you at a real question.

## Benchmark profiles correctly

Use a fresh worktree for each sample. Reusing one destination lets the first run warm the second and makes the comparison meaningless.

A good loop is:

1. Keep one source checkout warm and quiescent.
2. Keep source commit/toolchain/config identical across samples.
3. Create a new worktree at that revision.
4. Time `cargo warm seed --profile <profile>`.
5. Make one representative Rust edit in the package you care about.
6. Time the same compiler command your developers/agents normally use.
7. Repeat a few times and compare medians.

Measure at least two numbers:

```text
provisioning latency
first representative edit latency
```

A third useful number is a second edit in the same worktree, which tells you whether a slow first edit is a one-time relocation cost or the project's normal incremental cost.

### What to compare

Typical choices:

- `quick` vs `balanced` when the project is medium/large but has no important direct build script.
- `balanced` vs `deep` when a very large direct package has a build script and the first edit remains slower than the warm source checkout.
- cache cleanup vs no cleanup when the doctor reports an unusually large stale `deps/` population.

`deep` deliberately makes the selected package's own build script stale during provisioning. If that build script drives an expensive native toolchain, include that cost in the benchmark instead of hiding it.

## Interpreting results

Prefer the cheapest profile that gets the *agent-visible* first-edit latency close to the normal warm-source range.

If none of the profiles do:

1. run `doctor --probe`;
2. inspect build-script path blockers;
3. verify the source checkout was warmed with the same `cargo warm check` artifact family used by the destination;
4. check whether stale cache volume itself is hurting Cargo;
5. only then add explicit `seed-paths` or project-specific build controls.
