# cargo-warm

A Rust-first template for shipping command-line tools without rebuilding the same OSS plumbing every time.

Included from day one:

- Rust 2024 CLI structure with `clap`, tests, formatting, and strict Clippy checks
- pinned Rust toolchain for reproducible local and CI builds
- `dist`-generated GitHub Releases for macOS, Linux, and Windows
- shell, PowerShell, and npm installers generated from the same release artifacts
- GitHub artifact attestations
- a self-contained Astro/ZueDocs site under `docs/`
- Vercel affected-path filtering so Rust-only commits do not deploy docs
- one bootstrap command for project, npm, docs, license, and toolchain identity
- repo-local release instructions for coding agents

## Start a new CLI

Create the repository from this GitHub template:

```bash
gh repo create acme/pluck \
  --public \
  --template amxv/cargo-warm \
  --clone
cd pluck
```

Initialize its identity:

```bash
just bootstrap \
  --cli-name pluck \
  --npm-package @acme/pluck \
  --description "A fast file picker" \
  --license Apache-2.0 \
  --rust-version 1.98.0
```

The GitHub owner/repository are inferred from `origin` when the repo was created from the template. The Cargo package defaults to the CLI name. Pass `--crate-name` only when the crate and executable genuinely need different names.

To make the crate publishable on crates.io as well, add `--crates-io` after confirming the crate name is available. The default is disabled so cloning the template cannot accidentally publish a placeholder package.

## Local development

```bash
just check-fast
just check
just build
just run --help
just run hello agent
```

The underlying commands remain ordinary Cargo commands; the `Justfile` is only a memorable project surface.

## Project layout

```text
Cargo.toml              package identity, dependencies, lints
Cargo.lock              reproducible application dependency graph
rust-toolchain.toml     pinned Rust toolchain
src/main.rs             process boundary only
src/lib.rs              reusable application entrypoint
src/cli.rs              clap command model
src/commands/           command implementations
tests/                  CLI integration tests
dist-workspace.toml               release/install distribution config
scripts/bootstrap.py    one-time/re-runnable identity setup
docs/                   isolated Astro/ZueDocs application
.github/workflows/      CI, docs CI, generated release workflow
.agents/skills/release/ release checklist for future agents
```

JavaScript dependencies, Astro state, and Vercel configuration live entirely under `docs/`; the repository root stays focused on Rust.

## Docs

```bash
just docs-install
just docs-dev
just docs-check
just docs-build
```

For Vercel, configure the project Root Directory as `docs`. `docs/vercel.json` skips builds when a push contains no `docs/` changes.

## Distribution

`dist-workspace.toml` pins `cargo-dist` and generates:

- GitHub release archives for Apple Silicon + Intel macOS, ARM64 + x64 Linux, and x64 Windows
- `curl | sh` installer
- PowerShell installer
- generated npm binary package
- checksums and GitHub artifact attestations

Inspect a release plan with:

```bash
dist plan
```

Regenerate the release workflow after changing `dist-workspace.toml`:

```bash
dist generate
```

The generated `.github/workflows/release.yml` is committed. Do not hand-edit it; change `dist-workspace.toml` and regenerate instead.

## Release

Update the version in `Cargo.toml`, update `docs/src/content/docs/changelog.md`, run the checks, commit and push, then tag the exact Cargo version:

```bash
just check
just docs-check
just docs-build
just release-tag 0.2.0
```

The tag triggers the generated `dist` workflow. GitHub Releases and installers are published automatically. npm publishing requires a repository secret named `NPM_TOKEN`.

See `.agents/skills/release/SKILL.md` and `CONTRIBUTORS.md` for the full release contract.

## License

Apache-2.0
