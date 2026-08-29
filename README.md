# cargo-warm

`cargo-warm` makes new Rust worktrees inherit useful build state from an already-warm checkout without sharing one mutable Cargo directory.

It is built for developers, IDEs, and coding agents that create worktrees frequently and want the first compiler feedback to feel much closer to an already-warm workspace.

## Install

```bash
cargo install cargo-warm --locked
```

Use it as either `cargo warm …` or `cargo-warm …`.

## Quickstart

Start with the doctor:

```bash
cargo warm doctor
```

It inspects the project, Cargo cache layout, Rust toolchain, build scripts, and available warm worktrees. It recommends a profile and prints a starter `.agents/.cargo-warm.toml`.

A typical config is:

```toml
version = 1
default-profile = "balanced"
manifests = ["Cargo.toml"]
unstable-bootstrap = true # stable/beta only; see docs
```

Then put one command in your worktree creation hook:

```bash
cargo warm seed
```

Cargo-warm finds a nearby compatible warm checkout and forks its build state into the new worktree's own writable cache.

## Profiles

| Profile | Startup behavior |
| --- | --- |
| `quick` | Fork cache state and repair safe freshness metadata. |
| `balanced` | Also open inherited relocatable rustc incremental state before the first edit. |
| `deep` | Also re-establish the selected package's own build-script boundary. |

`cargo warm doctor` recommends a starting profile and tells you which alternatives are worth benchmarking.

Projects can define custom profiles in `.agents/.cargo-warm.toml` by inheriting a built-in profile and overriding manifests, seed paths, target inclusion, or priming behavior.

For `balanced` and `deep`, use the same relocatable compiler family in the warm source checkout and agent worktrees:

```bash
cargo warm check --manifest-path Cargo.toml
```

Rust 1.98+ is required for relocatable checks. Nightly/dev can use the compiler capability directly; stable/beta require an explicit project or CLI opt-in.

## The model

```text
warm checkout
     │
     │  APFS clone / filesystem reflink
     ▼
private worktree cache
     │
     └─ Cargo + rustc validate normally
```

The destination cache is independently writable. Cargo-warm treats inherited state as a starting point, never as proof that a build is correct. Unsafe or incompatible state falls back to normal Cargo/rustc revalidation.

## Commands

```bash
cargo warm doctor          # recommend configuration and diagnose misses
cargo warm seed            # fork warm state into a worktree
cargo warm check           # relocatable cargo check for workspace crates
cargo warm path            # show Cargo's resolved cache paths
cargo warm status          # show cargo-warm-owned caches
cargo warm gc --dry-run    # preview orphan cleanup
cargo warm gc              # remove orphaned cargo-warm-owned caches
```

Use `cargo warm doctor --probe` when you want Cargo's actual fingerprint rebuild reasons, and `--json` when another tool or agent should consume the report.

## Platform support

The fast copy-on-write path currently supports:

- macOS with APFS clone-on-write
- Linux filesystems with reflink support

A physical-copy fallback is available only when explicitly requested.

## Documentation

- [Quickstart](https://cargowarm.ashray.xyz/docs/quickstart)
- [Profiles and configuration](https://cargowarm.ashray.xyz/docs/profiles-and-config)
- [Doctor and benchmarking](https://cargowarm.ashray.xyz/docs/doctor-and-benchmarking)
- [Worktree hooks](https://cargowarm.ashray.xyz/docs/worktree-hooks)
- [How it works](https://cargowarm.ashray.xyz/docs/how-it-works)
- [Architecture](https://cargowarm.ashray.xyz/docs/architecture)

## License

Apache-2.0
