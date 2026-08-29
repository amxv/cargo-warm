---
title: Command reference
description: A compact map of the starter CLI and repository commands.
order: 4
category: Reference
summary: Cargo, Just, docs, bootstrap, and distribution commands in one place.
---

## Starter CLI

```bash
cargo-warm --help
cargo-warm --version
cargo-warm hello
cargo-warm hello agent
```

## Rust development

```bash
just check-fast
just check
just build
just build-release
just run --help
```

Equivalent primitives remain available directly:

```bash
cargo fmt --all -- --check
cargo check --locked
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

## Docs

```bash
just docs-install
just docs-dev
just docs-check
just docs-build
just docs-test
```

## Distribution

```bash
dist plan
dist generate
just release-tag 0.2.0
```

## Bootstrap

```bash
just bootstrap --help
just bootstrap \
  --cli-name pluck \
  --npm-package @acme/pluck \
  --description "A fast file picker"
```
