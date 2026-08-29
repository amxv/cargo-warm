#!/usr/bin/env python3
"""Initialize a project cloned from rust-cli-template."""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from urllib.parse import urlparse

ROOT = Path(__file__).resolve().parent.parent
CARGO_TOML = ROOT / "Cargo.toml"
DIST_TOML = ROOT / "dist-workspace.toml"
TOOLCHAIN_TOML = ROOT / "rust-toolchain.toml"
ASTRO_CONFIG = ROOT / "docs" / "astro.config.mjs"
APACHE_LICENSE = ROOT / "scripts" / "licenses" / "Apache-2.0.txt"

TEMPLATE_OWNER = "amxv"
TEMPLATE_REPO = "rust-cli-template"
TEMPLATE_CLI = "mycli"
TEMPLATE_NPM = "@amxv/rust-cli-template"

LICENSES = {
    "Apache-2.0": ("Apache", "Apache-2.0"),
    "MIT": ("MIT", "MIT"),
    "BSD-3-Clause": ("BSD", "BSD-3-Clause"),
    "ISC": ("ISC", "ISC"),
    "MPL-2.0": ("MPL-2", "MPL-2.0"),
    "GPL-3.0-only": ("GPL-3", "GPL-3.0-only"),
    "AGPL-3.0-only": ("AGPL", "AGPL-3.0-only"),
    "Unlicense": ("Unlicense", "Unlicense"),
}
LICENSE_ALIASES = {
    "apache": "Apache-2.0",
    "apache2": "Apache-2.0",
    "apache-2.0": "Apache-2.0",
    "mit": "MIT",
    "bsd": "BSD-3-Clause",
    "bsd-3-clause": "BSD-3-Clause",
    "isc": "ISC",
    "mpl2": "MPL-2.0",
    "mpl-2.0": "MPL-2.0",
    "gpl3": "GPL-3.0-only",
    "gpl-3.0": "GPL-3.0-only",
    "agpl": "AGPL-3.0-only",
    "agpl-3.0": "AGPL-3.0-only",
    "unlicense": "Unlicense",
}


@dataclass(frozen=True)
class State:
    crate_name: str
    cli_name: str
    github_owner: str
    github_repo: str
    npm_scope: str | None
    npm_package: str
    description: str
    homepage: str
    canonical_url: str
    license: str
    rust_version: str
    crates_io: bool

    @property
    def github_url(self) -> str:
        return f"https://github.com/{self.github_owner}/{self.github_repo}"

    @property
    def npm_full_name(self) -> str:
        if self.npm_scope:
            return f"{self.npm_scope}/{self.npm_package}"
        return self.npm_package

    @property
    def crate_ident(self) -> str:
        return self.crate_name.replace("-", "_")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Initialize Rust CLI, repository, npm, docs, license, and toolchain identity."
    )
    parser.add_argument("--cli-name", help="User-facing binary/command name")
    parser.add_argument("--crate-name", help="Cargo package name; defaults to --cli-name")
    parser.add_argument("--github-owner", help="GitHub user or organization")
    parser.add_argument("--github-repo", help="GitHub repository name")
    parser.add_argument("--npm-package", help="npm package, e.g. @acme/pluck or pluck")
    parser.add_argument("--description", help="One-line project description")
    parser.add_argument("--homepage", help="Project homepage URL")
    parser.add_argument("--canonical-url", help="Canonical docs-site URL")
    parser.add_argument("--license", help="SPDX license identifier; Apache-2.0 by default")
    parser.add_argument("--rust-version", help="Pinned Rust toolchain, e.g. 1.98.0")
    crates = parser.add_mutually_exclusive_group()
    crates.add_argument("--crates-io", action="store_true", dest="crates_io")
    crates.add_argument("--no-crates-io", action="store_false", dest="crates_io")
    parser.set_defaults(crates_io=None)
    parser.add_argument("--visibility", choices=("public", "private"), help="Warning context only")
    return parser.parse_args()


def read_toml(path: Path) -> dict:
    with path.open("rb") as file:
        return tomllib.load(file)


def parse_github_url(value: str) -> tuple[str, str] | None:
    match = re.search(r"github\.com[:/]([^/]+)/([^/#]+?)(?:\.git)?$", value.strip())
    if not match:
        return None
    return match.group(1), match.group(2)


def detected_origin() -> tuple[str, str] | None:
    result = subprocess.run(
        ["git", "remote", "get-url", "origin"],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    if result.returncode != 0:
        return None
    return parse_github_url(result.stdout)


def current_state() -> State:
    cargo = read_toml(CARGO_TOML)
    package = cargo["package"]
    bins = cargo.get("bin", [])
    cli_name = bins[0]["name"] if bins else package["name"]
    repository = package["repository"]
    github = parse_github_url(repository)
    if not github:
        raise SystemExit(f"Cargo.toml repository is not a GitHub URL: {repository}")

    dist = read_toml(DIST_TOML)["dist"]
    npm_scope = dist.get("npm-scope") or None
    npm_package = dist.get("npm-package", package["name"])

    toolchain = read_toml(TOOLCHAIN_TOML)["toolchain"]["channel"]
    astro = ASTRO_CONFIG.read_text()
    site_match = re.search(r'site:\s*"([^"]+)"', astro)
    canonical = site_match.group(1) if site_match else package.get("homepage", repository)

    return State(
        crate_name=package["name"],
        cli_name=cli_name,
        github_owner=github[0],
        github_repo=github[1],
        npm_scope=npm_scope,
        npm_package=npm_package,
        description=package["description"],
        homepage=package.get("homepage", repository),
        canonical_url=canonical,
        license=package.get("license", "Apache-2.0"),
        rust_version=toolchain,
        crates_io=package.get("publish", True) is not False,
    )


def normalize_license(value: str) -> str:
    raw = value.strip()
    key = raw.lower().replace("_", " ").replace(" ", "-")
    key = re.sub(r"-+", "-", key)
    if raw in LICENSES:
        return raw
    if key in LICENSE_ALIASES:
        return LICENSE_ALIASES[key]
    for spdx in LICENSES:
        if spdx.lower() == raw.lower():
            return spdx
    raise SystemExit(f"Unsupported license {value!r}. Choose: {', '.join(LICENSES)}")


def parse_npm(value: str) -> tuple[str | None, str]:
    if value.startswith("@"):
        match = re.fullmatch(r"(@[a-z0-9][a-z0-9._~-]*)/([a-z0-9][a-z0-9._~-]*)", value)
        if not match:
            raise SystemExit(f"Invalid scoped npm package: {value}")
        return match.group(1), match.group(2)
    if not re.fullmatch(r"[a-z0-9][a-z0-9._~-]*", value):
        raise SystemExit(f"Invalid npm package: {value}")
    return None, value


def resolve_state(current: State, args: argparse.Namespace) -> State:
    origin = detected_origin()
    cloned_from_template = (
        current.github_owner == TEMPLATE_OWNER
        and current.github_repo == TEMPLATE_REPO
        and origin is not None
        and origin != (TEMPLATE_OWNER, TEMPLATE_REPO)
    )
    owner = args.github_owner or (origin[0] if cloned_from_template else current.github_owner)
    repo = args.github_repo or (origin[1] if cloned_from_template else current.github_repo)

    cli_name = args.cli_name or (
        repo if cloned_from_template and current.cli_name == TEMPLATE_CLI else current.cli_name
    )
    crate_name = args.crate_name or (
        cli_name if current.crate_name == TEMPLATE_CLI else current.crate_name
    )

    if args.npm_package:
        npm_scope, npm_package = parse_npm(args.npm_package)
    elif cloned_from_template and current.npm_full_name == TEMPLATE_NPM:
        npm_scope, npm_package = parse_npm(f"@{owner.lower()}/{repo.lower()}")
    else:
        npm_scope, npm_package = current.npm_scope, current.npm_package

    github_url = f"https://github.com/{owner}/{repo}"
    homepage = args.homepage or (
        f"{github_url}#readme" if cloned_from_template else current.homepage
    )
    canonical = args.canonical_url or (
        github_url if cloned_from_template else current.canonical_url
    )

    state = State(
        crate_name=crate_name,
        cli_name=cli_name,
        github_owner=owner,
        github_repo=repo,
        npm_scope=npm_scope,
        npm_package=npm_package,
        description=args.description or current.description,
        homepage=homepage,
        canonical_url=canonical,
        license=normalize_license(args.license or current.license),
        rust_version=args.rust_version or current.rust_version,
        crates_io=current.crates_io if args.crates_io is None else args.crates_io,
    )
    validate_state(state)
    if args.visibility == "private":
        print(
            "Warning: dist installers download GitHub Release assets anonymously by default; "
            "a private repository needs an authenticated artifact strategy.",
            file=sys.stderr,
        )
    return state


def validate_state(state: State) -> None:
    cargo_name = r"[a-zA-Z0-9](?:[a-zA-Z0-9_-]*[a-zA-Z0-9])?"
    if not re.fullmatch(cargo_name, state.crate_name):
        raise SystemExit(f"Invalid Cargo package name: {state.crate_name}")
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]*", state.cli_name):
        raise SystemExit(f"Invalid CLI name: {state.cli_name}")
    if not re.fullmatch(r"[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?", state.github_owner):
        raise SystemExit(f"Invalid GitHub owner: {state.github_owner}")
    if not re.fullmatch(r"[A-Za-z0-9._-]+", state.github_repo):
        raise SystemExit(f"Invalid GitHub repository: {state.github_repo}")
    if not re.fullmatch(r"\d+\.\d+(?:\.\d+)?", state.rust_version):
        raise SystemExit(f"Invalid Rust version: {state.rust_version}")
    for label, value in (("homepage", state.homepage), ("canonical URL", state.canonical_url)):
        parsed = urlparse(value)
        if parsed.scheme not in {"http", "https"} or not parsed.netloc:
            raise SystemExit(f"{label} must be an absolute http(s) URL: {value}")


def replace_assignment(text: str, key: str, value: str, *, quoted: bool = True) -> str:
    rendered = f'"{value}"' if quoted else value
    pattern = rf"(?m)^(\s*{re.escape(key)}\s*=\s*).*$"
    updated, count = re.subn(pattern, rf"\g<1>{rendered}", text, count=1)
    if count != 1:
        raise SystemExit(f"Could not find {key} in expected config")
    return updated


def update_cargo(current: State, new: State) -> None:
    text = CARGO_TOML.read_text()
    text = replace_assignment(text, "name", new.crate_name)
    text = replace_assignment(text, "rust-version", new.rust_version)
    text = replace_assignment(text, "description", new.description)
    text = replace_assignment(text, "license", new.license)
    text = replace_assignment(text, "repository", new.github_url)
    text = replace_assignment(text, "homepage", new.homepage)
    text = replace_assignment(text, "publish", "true" if new.crates_io else "false", quoted=False)

    bin_pattern = rf'(?s)(\[\[bin\]\]\s*name\s*=\s*)"{re.escape(current.cli_name)}"'
    text, count = re.subn(bin_pattern, rf'\g<1>"{new.cli_name}"', text, count=1)
    if count != 1:
        raise SystemExit("Could not update [[bin]] name in Cargo.toml")
    CARGO_TOML.write_text(text)


def update_dist(new: State) -> None:
    text = DIST_TOML.read_text()
    text = replace_assignment(text, "npm-package", new.npm_package)
    scope_pattern = r'(?m)^npm-scope\s*=.*\n'
    if new.npm_scope:
        if re.search(scope_pattern, text):
            text = re.sub(scope_pattern, f'npm-scope = "{new.npm_scope}"\n', text, count=1)
        else:
            text = text.replace(
                f'npm-package = "{new.npm_package}"\n',
                f'npm-scope = "{new.npm_scope}"\nnpm-package = "{new.npm_package}"\n',
                1,
            )
    else:
        text = re.sub(scope_pattern, "", text, count=1)
    DIST_TOML.write_text(text)


def update_toolchain(new: State) -> None:
    text = TOOLCHAIN_TOML.read_text()
    TOOLCHAIN_TOML.write_text(replace_assignment(text, "channel", new.rust_version))


def update_source_identity(current: State, new: State) -> None:
    main = ROOT / "src" / "main.rs"
    text = main.read_text().replace(f"{current.crate_ident}::run()", f"{new.crate_ident}::run()")
    main.write_text(text)

    test = ROOT / "tests" / "cli.rs"
    text = test.read_text().replace(
        f"CARGO_BIN_EXE_{current.cli_name}", f"CARGO_BIN_EXE_{new.cli_name}"
    )
    test.write_text(text)


def broad_identity_files() -> list[Path]:
    files = [ROOT / "README.md", ROOT / "AGENTS.md", ROOT / "CONTRIBUTORS.md"]
    files.extend((ROOT / ".agents").rglob("*.md"))
    for suffix in ("*.md", "*.ts", "*.astro", "*.json", "*.mjs"):
        files.extend((ROOT / "docs").rglob(suffix))
    return sorted(set(path for path in files if path.exists()))


def replace_identity_text(current: State, new: State) -> None:
    replacements = [
        (current.npm_full_name, new.npm_full_name),
        (current.github_url, new.github_url),
        (current.homepage, new.homepage),
        (current.canonical_url, new.canonical_url),
        (current.description, new.description),
        (current.github_repo, new.github_repo),
        (current.cli_name, new.cli_name),
        (current.license, new.license),
        (current.rust_version, new.rust_version),
    ]
    replacements = sorted(
        ((old, fresh) for old, fresh in replacements if old and fresh and old != fresh),
        key=lambda pair: len(pair[0]),
        reverse=True,
    )
    for path in broad_identity_files():
        text = path.read_text()
        updated = text
        for old, fresh in replacements:
            updated = updated.replace(old, fresh)
        if path == ROOT / "docs" / "package.json":
            updated = updated.replace(
                f'@{current.github_owner.lower()}/{current.github_repo.lower()}-docs',
                f'@{new.github_owner.lower()}/{new.github_repo.lower()}-docs',
            )
        if updated != text:
            path.write_text(updated)

    docs_package = ROOT / "docs" / "package.json"
    package_json = json.loads(docs_package.read_text())
    package_json["name"] = f"@{new.github_owner.lower()}/{new.github_repo.lower()}-docs"
    docs_package.write_text(json.dumps(package_json, indent=2) + "\n")

    docs_lock = ROOT / "docs" / "bun.lock"
    lock_text = docs_lock.read_text()
    lock_text, count = re.subn(
        r'("name":\s*)"[^"]+"',
        rf'\g<1>"{package_json["name"]}"',
        lock_text,
        count=1,
    )
    if count != 1:
        raise SystemExit("Could not update docs workspace identity in bun.lock")
    docs_lock.write_text(lock_text)

    astro = ASTRO_CONFIG.read_text()
    astro, count = re.subn(
        r'(site:\s*)"[^"]+"',
        rf'\g<1>"{new.canonical_url}"',
        astro,
        count=1,
    )
    if count != 1:
        raise SystemExit("Could not update docs canonical URL in astro.config.mjs")
    ASTRO_CONFIG.write_text(astro)


def render_license(state: State) -> str:
    if state.license == "Apache-2.0":
        return APACHE_LICENSE.read_text()
    author = os.environ.get("LICENSE_AUTHOR") or git_config("user.name") or state.github_owner
    year = os.environ.get("LICENSE_YEAR") or str(datetime.now().year)
    if state.license == "MIT":
        return f"""MIT License

Copyright (c) {year} {author}

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the \"Software\"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
"""

    generator = LICENSES[state.license][0]
    executable = shutil.which("license-generator")
    if not executable:
        raise SystemExit(
            f"{state.license} requires license-generator. Install it or choose Apache-2.0/MIT."
        )
    with tempfile.TemporaryDirectory() as tmp:
        output = Path(tmp) / "LICENSE"
        result = subprocess.run(
            [
                executable,
                generator,
                "--output",
                str(output),
                "--author",
                author,
                "--project",
                state.github_repo,
                "--year",
                year,
            ],
            cwd=ROOT,
            check=False,
        )
        if result.returncode != 0 or not output.exists():
            raise SystemExit(f"license-generator failed for {state.license}")
        return output.read_text()


def git_config(key: str) -> str:
    result = subprocess.run(
        ["git", "config", "--get", key],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    return result.stdout.strip() if result.returncode == 0 else ""


def refresh_lockfile() -> None:
    result = subprocess.run(["cargo", "generate-lockfile"], cwd=ROOT, check=False)
    if result.returncode != 0:
        raise SystemExit("cargo generate-lockfile failed after updating project identity")


def main() -> None:
    args = parse_args()
    current = current_state()
    new = resolve_state(current, args)
    license_text = render_license(new)

    update_cargo(current, new)
    update_dist(new)
    update_toolchain(new)
    update_source_identity(current, new)
    replace_identity_text(current, new)
    (ROOT / "LICENSE").write_text(license_text)
    refresh_lockfile()

    print(f"Initialized {new.cli_name} for {new.github_url}")
    print(f"Cargo package: {new.crate_name}")
    print(f"npm package: {new.npm_full_name}")
    print(f"Rust: {new.rust_version}")
    print(f"License: {new.license}")
    print(f"crates.io publishing: {'enabled' if new.crates_io else 'disabled'}")


if __name__ == "__main__":
    main()
