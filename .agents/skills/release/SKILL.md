---
name: release
description: Use this skill when the user says to cut, ship, publish, or create a new release for cargo-warm.
allowed-tools: Bash, Read, Write, Edit
---

# Release cargo-warm

Use this skill whenever the user asks for a new release.

`Cargo.toml` owns the package version. `docs/src/content/docs/changelog.md` owns curated release notes. `dist` owns native release artifacts, the shell installer, GitHub Release creation, checksums, and attestations. crates.io publication is a separate explicit channel.

## 1. Inspect release state

Read:

```text
Cargo.toml
dist-workspace.toml
docs/src/content/docs/changelog.md
```

Then inspect Git and releases:

```bash
git status --short --branch
git fetch origin --tags
git tag --sort=-version:refname | head -n 10
gh release list --limit 10
git log --oneline --decorate -n 30
```

Normally release from `main` after the intended commit is pushed.

## 2. Choose and set the version

Use an exact user-supplied version when provided. Otherwise choose the smallest SemVer bump justified by user-visible changes. Before 1.0, use a minor bump for breaking changes and a patch bump for compatible fixes/features unless the owner directs otherwise.

Update `Cargo.toml`, then refresh the lockfile and verify Cargo metadata reports the exact version:

```bash
cargo metadata --no-deps --format-version 1 \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["packages"][0]["version"])'
```

Do not reuse an existing tag or GitHub Release version.

## 3. Write the changelog

Add the newest entry first in `docs/src/content/docs/changelog.md`.

Focus on user-visible cache behavior, compatibility, performance, installation, command changes, and safety fixes. Omit internal refactors unless they materially affect users.

## 4. Verify release configuration

Use the exact `cargo-dist-version` pinned in `dist-workspace.toml`.

If distribution configuration changed, regenerate:

```bash
dist generate
```

Do not hand-edit `.github/workflows/release.yml`.

## 5. Validate

Run serially:

```bash
just check
just docs-check
just docs-build
dist plan
git diff --check
```

`dist plan` must describe the supported macOS/Linux archives, checksums, shell installer, release manifest, and attestations.

If the release is also intended for crates.io:

```bash
cargo publish --dry-run
```

Do not publish to crates.io merely because `publish = true`; it requires an explicit release decision and working registry ownership/authentication.

## 6. Commit and push release preparation

Review the final diff, commit release metadata, and push. Confirm the pushed commit is the exact commit intended for release.

## 7. Tag the Cargo version

```bash
just release-tag ${VERSION}
```

The helper verifies Cargo metadata before pushing `v${VERSION}`.

## 8. Watch the release workflow

```bash
gh run list --workflow release.yml --limit 5
gh run watch <run-id> --exit-status
```

Fix underlying failures before declaring the release complete.

## 9. Verify outputs

```bash
gh release view "v${VERSION}" --json tagName,name,url,assets
```

Confirm the release contains the artifacts described by `dist plan`.

When crates.io publication was explicitly requested, publish/verify it separately and report that status separately from GitHub Releases.

## 10. Report completion

Report the version/tag, release commit, changelog summary, validation results, GitHub Actions result, GitHub Release URL/artifacts, crates.io status when applicable, and any non-blocking warnings.
