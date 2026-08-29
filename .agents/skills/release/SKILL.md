---
name: release
description: Use this skill when the user says to cut, ship, publish, or create a new release for this Rust CLI.
allowed-tools: Bash, Read, Write, Edit
---

# Release this Rust CLI

Use this skill whenever the user asks for a new release.

`Cargo.toml` owns the package version. `docs/src/content/docs/changelog.md` owns curated release notes. `dist` owns release artifacts, installers, GitHub Release creation, attestations, and npm publication after the version tag reaches GitHub.

## 1. Inspect release state

Start from the repository root and read:

```text
Cargo.toml
dist-workspace.toml
docs/src/content/docs/changelog.md
```

Inspect Git/release state:

```bash
git status --short --branch
git fetch origin --tags
git tag --sort=-version:refname | head -n 10
gh release list --limit 10
git log --oneline --decorate -n 30
```

Identify the most recent release tag and review the commits/diff since it. Understand any uncommitted changes before editing release metadata.

Normally release from `main` with the branch pushed and synchronized with `origin/main` before the final tag is created.

## 2. Choose the version

If the user supplied an exact version, use it.

Otherwise choose the smallest SemVer bump justified by user-visible changes since the previous release:

- breaking compatibility change: major bump
- new backward-compatible functionality: minor bump
- fixes, performance improvements, or maintenance: patch bump
- before `1.0.0`, use a minor bump for breaking changes and a patch bump for compatible fixes

Do not reuse an existing tag, GitHub Release version, or npm version.

Update `package.version` in `Cargo.toml`, then run Cargo once without `--locked` so the root package entry in `Cargo.lock` follows the new version. Confirm the resolved package version:

```bash
cargo metadata --no-deps --format-version 1 \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["packages"][0]["version"])'
```

The value must exactly match the version about to be tagged.

## 3. Write the changelog

Update `docs/src/content/docs/changelog.md` manually using the actual changes since the previous release.

The new entry should:

- be first in the changelog
- use the exact version and current release date
- explain user-visible behavior, fixes, performance changes, compatibility changes, and installation changes
- combine related commits into concise release notes
- omit internal refactors and docs-only styling/navigation work unless they materially affect users
- avoid copying raw commit subjects verbatim

For a maintenance-only release with no direct user-visible behavior change, say so plainly rather than inventing features.

## 4. Verify release configuration

Read the pinned `cargo-dist` version from `dist-workspace.toml` and use that exact version.

If `dist-workspace.toml` changed since the previous committed release configuration, regenerate the release workflow before validation:

```bash
dist generate
```

The generated `.github/workflows/release.yml` should match the committed dist configuration.

When npm publishing is configured, confirm the GitHub secret exists without reading its value:

```bash
gh secret list --json name --jq 'any(.[]; .name == "NPM_TOKEN")'
```

A missing required publishing secret is a release blocker; configure it through the user's established secure credential path before tagging.

## 5. Validate

Run from the repository root, serially:

```bash
just check
just docs-check
just docs-build
dist plan
git diff --check
```

`dist plan` must describe the expected platform archives, checksums, shell installer, PowerShell installer, generated npm package, and release manifest.

If crates.io publishing is enabled in `Cargo.toml` and this release is intended for crates.io too, also run:

```bash
cargo publish --dry-run
```

Treat crates.io publication as a separate publication channel with its own authentication/trusted-publisher state.

## 6. Commit and push release preparation

Review the diff. Release preparation normally includes:

- `Cargo.toml`
- `Cargo.lock`
- `docs/src/content/docs/changelog.md`
- `dist-workspace.toml` and generated release workflow only when distribution configuration intentionally changed

Commit and push those changes to the release branch before tagging.

Confirm the pushed commit is the commit intended for release.

## 7. Tag the Cargo version

Create the tag through the repository helper:

```bash
just release-tag ${VERSION}
```

The helper verifies that `${VERSION}` matches Cargo metadata before pushing `v${VERSION}`.

Do not create an alternate tag merely to recover from a failed workflow. Fix the actual failure and retry the same release workflow/tag when possible.

## 8. Watch the release workflow

Find the run created by the pushed tag:

```bash
gh run list --workflow release.yml --limit 5
```

Watch the specific run through completion:

```bash
gh run watch <run-id> --exit-status
```

If it fails, inspect the failed job and fix the underlying issue before declaring the release complete.

## 9. Verify published outputs

Verify the GitHub Release and its assets:

```bash
gh release view "v${VERSION}" --json tagName,name,url,assets
```

Verify npm publication using the package identity from `dist-workspace.toml`:

```bash
npm view "<npm-package>" version dist-tags.latest --json
```

Confirm the published npm version equals `${VERSION}` and that the GitHub Release contains the artifacts described by `dist plan`.

If crates.io publication was requested, publish/verify it separately and report its status separately from GitHub/npm.

## 10. Report completion

Report:

- released version and tag
- release commit
- changelog summary
- validation results
- GitHub Actions release result
- GitHub Release URL and artifact set
- npm package/version
- crates.io status when applicable
- any non-blocking warnings
