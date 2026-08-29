import { afterEach, describe, expect, test } from "bun:test";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const script = join(dirname(fileURLToPath(import.meta.url)), "should-build.mjs");
const repos = [];

const run = (command, args, cwd) => {
  const result = spawnSync(command, args, {
    cwd,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"]
  });
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} failed\n${result.stdout}\n${result.stderr}`);
  }
  return result.stdout.trim();
};

const git = (cwd, ...args) => run("git", args, cwd);

const write = (root, relative, contents) => {
  const target = join(root, relative);
  mkdirSync(dirname(target), { recursive: true });
  writeFileSync(target, contents);
};

const commit = (root, message) => {
  git(root, "add", "-A");
  git(root, "commit", "-m", message);
  return git(root, "rev-parse", "HEAD");
};

const fixture = () => {
  const root = mkdtempSync(join(tmpdir(), "rust-cli-docs-vercel-"));
  repos.push(root);
  git(root, "init", "-q");
  git(root, "config", "user.name", "Template Test");
  git(root, "config", "user.email", "template-test@local.invalid");
  write(root, "docs/index.md", "docs v1\n");
  write(root, "src/lib.rs", "pub fn v1() {}\n");
  const base = commit(root, "base");
  return { root, docs: join(root, "docs"), base };
};

const decision = ({ docs, head, base }) =>
  spawnSync("node", [script], {
    cwd: docs,
    env: {
      ...process.env,
      VERCEL_GIT_COMMIT_SHA: head,
      VERCEL_GIT_PREVIOUS_SHA: base,
      VERCEL_GIT_PULL_REQUEST_BASE_SHA: "",
      SITE_DEPLOY_DIFF_BASE: ""
    },
    stdio: ["ignore", "pipe", "pipe"]
  }).status;

afterEach(() => {
  for (const repo of repos.splice(0)) rmSync(repo, { recursive: true, force: true });
});

describe("Vercel docs affected-path check", () => {
  test("builds for docs changes", () => {
    const repo = fixture();
    write(repo.root, "docs/index.md", "docs v2\n");
    const head = commit(repo.root, "docs edit");
    expect(decision({ ...repo, head })).toBe(1);
  }, 15_000);

  test("skips Rust-only changes", () => {
    const repo = fixture();
    write(repo.root, "src/lib.rs", "pub fn v2() {}\n");
    const head = commit(repo.root, "rust edit");
    expect(decision({ ...repo, head })).toBe(0);
  }, 15_000);

  test("fails open when the base is unavailable", () => {
    const repo = fixture();
    const head = repo.base;
    expect(decision({ ...repo, head, base: "missing-base" })).toBe(1);
  }, 15_000);
});
