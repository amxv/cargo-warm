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
cargo warm seed --copy-fallback
```

On modern Cargo versions with a separate `build_directory`, that directory is seeded by default. Final/link outputs in `target_directory` are skipped unless `--include-target` is requested.

The default fast path refuses to silently turn a multi-gigabyte COW operation into a physical copy. `--copy-fallback` is an explicit opt-in.

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

The next research layer is to diagnose those misses precisely and select or hydrate the nearest useful compiler state instead of treating one warm checkout as an opaque directory copy.

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
