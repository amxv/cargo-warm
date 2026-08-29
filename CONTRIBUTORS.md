# Contributor and maintainer notes

## Prerequisites

- Rust from `rust-toolchain.toml` (currently 1.98.0)
- `just`
- Bun for documentation work
- `dist` 0.32.0 when changing or inspecting release configuration
- GitHub repository admin access for release/npm/Vercel setup

## Development

```bash
just check-fast
just check
just build
./target/debug/cargo-warm --help
```

The full Rust check is deliberately conventional: format check, strict Clippy, then tests.

## Docs

The documentation application is isolated in `docs/`:

```bash
just docs-install
just docs-check
just docs-build
just docs-dev
```

Run `docs-check` and `docs-build` serially. Configure Vercel with Root Directory `docs`.

## Release infrastructure

`dist-workspace.toml` is maintained source. `.github/workflows/release.yml` is generated:

```bash
dist plan
dist generate
```

Current distribution defaults:

- GitHub Releases
- shell installer for macOS/Linux
- PowerShell installer for Windows
- npm binary installer package
- GitHub artifact attestations

## npm publishing

The generated `dist` npm publish job expects a GitHub Actions secret:

```text
NPM_TOKEN
```

For the first publish, the token normally needs write access to the scope because the npm package does not exist yet. Store credentials outside the repository.

## crates.io

The template itself sets `publish = false`. A real project can opt in during bootstrap with `--crates-io`, after checking that its crate name is available. That makes normal `cargo publish` / `cargo install <crate>` distribution possible; crates.io publisher/trusted-publishing setup remains an explicit owner action.

## Release process

The repository-local release skill is the canonical release procedure:

```text
.agents/skills/release/SKILL.md
```

Use it whenever cutting, shipping, or publishing a new release.
