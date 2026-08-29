# Contributor and maintainer notes

## Prerequisites

- Rust from `rust-toolchain.toml` (currently 1.98.0)
- `just`
- Bun for documentation work
- `dist` 0.32.0 when changing release configuration

## Development

```bash
just check-fast
just check
just build
./target/debug/cargo-warm --help
```

For cache behavior, prefer temporary repositories and isolated `XDG_CACHE_HOME` values. Do not test GC against a developer's real registry.

## Docs

```bash
just docs-install
just docs-check
just docs-build
just docs-dev
```

The documentation application is isolated under `docs/`. Run `docs-check` and `docs-build` serially.

## Release infrastructure

`dist-workspace.toml` is maintained source and `.github/workflows/release.yml` is generated:

```bash
dist plan
dist generate
```

Current release targets are Apple Silicon and Intel macOS plus ARM64 and x64 Linux. The release graph produces GitHub Release archives, checksums, a shell installer, and artifact attestations.

## crates.io

The crate is configured as publishable so the public project can support normal Rust installation:

```bash
cargo install cargo-warm
```

Before the first publication, run `cargo publish --dry-run` and establish crates.io ownership/trusted publishing explicitly. Do not publish while the repository is intentionally private unless the owner asks for it.

## Release process

The repository-local release skill is canonical:

```text
.agents/skills/release/SKILL.md
```
