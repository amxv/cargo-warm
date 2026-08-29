---
title: Quickstart
description: Go from a fresh clone to a checked Rust CLI and local docs site.
order: 1
category: Start
summary: The shortest path from template clone to product code.
---

## Create the repository

```bash
gh repo create acme/pluck \
  --public \
  --template amxv/rust-cli-template \
  --clone
cd pluck
```

## Initialize project identity

```bash
just bootstrap \
  --cli-name pluck \
  --npm-package @acme/pluck \
  --description "A fast file picker" \
  --license Apache-2.0 \
  --rust-version 1.98.0
```

The bootstrap script detects the new GitHub origin and synchronizes Cargo, `dist`, docs, npm, license, and toolchain metadata.

## Run the starter

```bash
just check
cargo run -- --help
cargo run -- hello agent
```

The starter command is intentionally tiny. Replace it rather than building abstractions around the greeting example.

## Run the docs site

```bash
just docs-install
just docs-dev
```

The docs application lives completely under `docs/`, including its dependencies and Vercel configuration.
