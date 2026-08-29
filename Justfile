set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

default:
    @just --list

bootstrap *args:
    python3 scripts/bootstrap.py {{args}}

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

check-fast:
    cargo fmt --all -- --check
    cargo check --locked
    cargo clippy --locked --all-targets -- -D warnings

check:
    cargo fmt --all -- --check
    cargo clippy --locked --all-targets -- -D warnings
    cargo test --locked

build:
    cargo build --locked

build-release:
    cargo build --locked --release

run *args:
    cargo run -- {{args}}

docs-install:
    cd docs && bun install --frozen-lockfile

docs-dev:
    cd docs && bun run dev

docs-check:
    cd docs && bun run check

docs-build:
    cd docs && bun run build

docs-test:
    cd docs && bun run test:vercel

dist-plan:
    dist plan

dist-generate:
    dist generate

release-tag version:
    #!/usr/bin/env bash
    set -euo pipefail
    actual="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["packages"][0]["version"])')"
    if [[ "$actual" != "{{version}}" ]]; then
      echo "Cargo.toml version is $actual, not {{version}}" >&2
      exit 1
    fi
    git tag "v{{version}}"
    git push origin "v{{version}}"
