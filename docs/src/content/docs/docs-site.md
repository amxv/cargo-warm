---
title: Docs site maintenance
description: Maintain the Astro/ZueDocs site without mixing JavaScript tooling into the Rust root.
order: 5
category: Reference
summary: The docs workspace boundary, checks, and Vercel behavior.
---

## Workspace boundary

Everything required by the web app lives in `docs/`:

```text
docs/package.json
docs/bun.lock
docs/astro.config.mjs
docs/vercel.json
docs/scripts/
docs/src/
```

The repository root intentionally has no Node package or Astro configuration.

## Development

```bash
just docs-install
just docs-dev
```

## Validation

```bash
just docs-check
just docs-build
```

Run check and build serially.

## Vercel

Set the Vercel project Root Directory to `docs`. The committed `vercel.json` builds static Astro output and calls `scripts/should-build.mjs` before a deployment.

That script compares Git commits and returns a skip result when only Rust/root code changed. Its behavior is covered by `bun run test:vercel`.
