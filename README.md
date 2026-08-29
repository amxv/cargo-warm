# cargo-warm

`cargo-warm` makes new Rust worktrees start closer to warm by forking Cargo build state from an already-warm checkout into a separate writable cache.

It is built for developers and coding agents that create Git worktrees frequently and do not want every new checkout to rebuild the same dependency graph from scratch.

## Why

A common workflow looks like this:

```text
main @ A                  warm Cargo state
   |
   +-- feature worktree   same starting revision, cold build state
```

Pointing both worktrees at one mutable build directory is fast until concurrent Cargo processes contend on the same state or one checkout invalidates another. `cargo-warm` takes a different approach:

```text
warm source cache
       |
       | APFS clone / filesystem reflink
       v
private destination cache
       |
       +-- Cargo and rustc validate freshness normally
```

The destination is independently writable. The initial filesystem blocks are shared copy-on-write where the platform supports it.

## Install

Install from crates.io:

```bash
cargo install cargo-warm --locked
```

The binary is a Cargo subcommand, so both forms work:

```bash
cargo warm --help
cargo-warm --help
```

## Worktree hook

From a newly-created Git worktree whose repository also has a `main` worktree:

```bash
cargo warm seed
```

`cargo-warm` discovers the separate `main` checkout, asks Cargo for the actual build-cache paths, and seeds this checkout's private cache.

For editors, agent runtimes, or custom worktree managers, use the explicit primitive:

```bash
cargo warm seed \
  --from /path/to/warm/main \
  --to /path/to/new/worktree
```

Repositories with multiple Cargo workspaces can repeat `--manifest-path`:

```bash
cargo warm seed \
  --manifest-path src-tauri/Cargo.toml \
  --manifest-path src-tauri/sidecars/server/Cargo.toml
```

This makes the command easy to call from arbitrary worktree-init scripts without coupling the tool to a particular IDE or agent framework.

## Commands

```bash
cargo warm path
cargo warm seed
cargo warm doctor
cargo warm check
cargo warm status
cargo warm gc
```

### `path`

Shows the `build_directory` and `target_directory` Cargo resolves for a workspace. `cargo-warm` uses Cargo metadata rather than reverse-engineering workspace path hashes.

```bash
cargo warm path --workspace /path/to/repo
cargo warm path --json
```

### `seed`

Forks warm state into the destination's isolated cache.

```bash
cargo warm seed
cargo warm seed --from ../main --to .
cargo warm seed --include-target
cargo warm seed --prime --unstable-bootstrap
cargo warm seed --seed-path path/to/native/cache
cargo warm seed --no-freshness-rebase
cargo warm seed --copy-fallback
```

On modern Cargo versions with a separate `build_directory`, that directory is seeded by default. Final/link outputs in `target_directory` are skipped unless `--include-target` is requested.

When `--from` is omitted, cargo-warm ranks active worktrees by Git graph distance, including nearby sibling branches. It skips worktrees that are compiling, skips incompatible toolchains, and prefers candidates that actually contain warm cache roots rather than assuming `main` is always the best seed.

Build scripts sometimes link ignored native outputs that live outside Cargo's own build directory. For ordinary `rustc-link-search` + `rustc-link-lib` outputs, cargo-warm discovers those automatically and forks only the final linkable artifacts plus any required workspace-relative symlink. It deliberately avoids copying opaque native compiler caches such as Swift/Clang module caches. `--seed-path` remains a repeatable escape hatch for unusual portable native state.

After cloning, cargo-warm rebases Cargo freshness conservatively: clean tracked files must have the same Git blob in both worktrees, watched build-script trees must be byte-equivalent, and checkout-local build-script paths must have a safe equivalent in the destination. Use `--no-freshness-rebase` to disable this layer.

For agent worktrees where the first *edited* compile matters more than setup latency, `--prime` pays rustc's one-time relocation-validation cost during seeding. Cargo-warm temporarily advances the mtimes of the direct package's root Rust target and, when present, that package's own `custom-build` target (`build.rs`) without changing a byte. It then runs the same relocatable `cargo warm check` and restores the exact original timestamps. Path dependencies are deliberately left untouched, so priming one package does not wake unrelated native build scripts. On stable/beta, pair it with the same explicit `--unstable-bootstrap` acknowledgement required by `cargo warm check`.

On Golden Goose's ~300k-line Rust application, the measured progression was:

```text
old 3A first relocated check:       ~11m01s
3B exact new-worktree check:          2.56s
0.2.0 released-hook first edit:      59.22s
warm-main comparison edit:           37.04s
0.2.1 seed + package prime:     27.40s + 79.80s
0.2.1 first edit after prime:        41.47s
```

That is why `--prime` is opt-in rather than unconditional. Small and medium crates may already get near-warm first-edit behavior directly from the forked incremental state; very large monolithic crates can choose to move the one-time validation cost into worktree provisioning so the agent starts hot. Priming may intentionally rerun the selected package's own build script, so projects with expensive custom builds should decide whether that startup tradeoff is worthwhile.

The default fast path refuses to silently turn a multi-gigabyte COW operation into a physical copy. `--copy-fallback` is an explicit opt-in.

### `doctor`

Explains whether a worktree is a good cache-fork candidate before you spend time on a first compile:

```bash
cargo warm doctor
cargo warm doctor --from /warm/main --to /new/worktree
cargo warm doctor --json
```

For clean worktrees at the same Git revision it reports tracked-file mtime skew, resolved build-cache paths, and local build scripts. This separates a common Cargo freshness problem from deeper rustc incremental reuse problems.

Add `--probe` when you want the actual Cargo rebuild reasons:

```bash
cargo warm doctor --probe
```

Probe mode runs `cargo check` with Cargo's fingerprint diagnostics enabled and classifies changed-file, build-script, dependency, environment, compiler, and filesystem misses. It can compile code when the destination is not fresh, so the non-probe mode remains the cheap default.

### `check` (experimental)

`cargo warm check` runs `cargo check` with a workspace-only rustc wrapper that opts local crates into rustc's relocatable incremental working-directory mode. Warm the source checkout with the same command before seeding a worktree:

```bash
# Warm main once in the relocatable artifact family.
cargo warm check

# New worktree startup.
cargo warm seed

# First compiler feedback in the worktree.
cargo warm check
```

Rust 1.98 added the incremental behavior this relies on, but the compiler flag is still unstable. Nightly/dev toolchains can use it directly. On stable/beta, cargo-warm refuses to enable `RUSTC_BOOTSTRAP` silently; experimentation requires an explicit acknowledgement:

```bash
cargo warm check --unstable-bootstrap --workspace
```

Stable bootstrap is scoped to the one workspace crate being compiled, and cargo-warm passes compiler guards that forbid unstable source features. `RUSTC_BOOTSTRAP` is still visible to Rust's `env!` / `option_env!` macros, so stable-toolchain bootstrap mode remains explicit rather than silently changing compilation inputs. Arguments after cargo-warm's options are forwarded to `cargo check`.

### `status`

Shows cache roots created by `cargo-warm` and whether their destination worktrees are still available.

### `gc`

Removes only cache roots recorded by `cargo-warm` whose destination worktrees no longer exist.

```bash
cargo warm gc --dry-run
cargo warm gc
```

## Safety model

- Cargo and rustc remain the freshness and correctness authority.
- For worktrees in the same Git repository, cargo-warm may mirror mtimes only for clean tracked files whose Git blob identity is identical in source and destination. This removes checkout-time false invalidation without hiding edits from Cargo.
- Cached build-script directives that contain the source checkout are rebased only for supported path fields whose equivalent destination path exists. Unknown or missing native paths block freshness rebasing rather than leaking state across worktrees.
- Experimental relocatable checks still invoke rustc in the destination checkout; path-sensitive compiler inputs are revalidated rather than copied as trusted answers.
- Source and destination build directories must be different paths.
- Active source or destination compiler processes cause seeding to fail rather than copy a torn cache.
- Cargo and rustc identities must match between source and destination.
- Cache publication uses a temporary destination plus rename.
- `gc` only deletes cache roots that `cargo-warm` created and recorded.
- macOS uses APFS clone-on-write.
- Linux requires filesystem reflink support for the default fast path.
- No shared mutable Cargo build directory is introduced.

## What 0.1 solves

The current implementation is the practical cache-fork layer. It can preserve expensive dependency artifacts, build-script output, fingerprints, and rustc incremental state when Cargo considers them reusable after relocation.

It does not yet make every nearby branch behave exactly like an already-warm compiler session. Cargo build-script mtimes, path-bearing build outputs, changed source, feature/configuration differences, and rustc incremental invalidation can still make workspace-local crates rebuild.

`cargo warm doctor` now exposes those misses directly. The deeper research layer is making rustc's local incremental state more relocatable while still letting Cargo invoke rustc in the destination checkout, so path-sensitive inputs are validated instead of bypassed.

## Platform support

The fast COW path currently targets:

- macOS on APFS
- Linux filesystems with reflink support

Windows releases are intentionally not advertised until there is a native safe clone strategy and process-quiescence implementation.

## Development

```bash
just check-fast
just check
just build
```

Docs live independently under `docs/`:

```bash
just docs-install
just docs-check
just docs-build
```

## Release infrastructure

`dist-workspace.toml` generates native GitHub Release archives, checksums, shell installers, and artifact attestations for supported macOS/Linux targets. The same version is published to crates.io for `cargo install cargo-warm`.

## License

Apache-2.0
