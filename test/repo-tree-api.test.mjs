import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, mkdir, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { repoTreeFiles } from "../src/adapters/http/repo-tree-api.mjs";

test("uses git ls-files when the target is a git work tree", async () => {
  const { files, source } = await repoTreeFiles("/repo", {
    gitAvailableFn: () => true,
    gitFn: () => "src/a.js\nsrc/b/c.js\nREADME.md\n"
  });
  assert.equal(source, "git");
  assert.deepEqual(files, ["src/a.js", "src/b/c.js", "README.md"]);
});

test("falls back to the filesystem walk when not a git repo", async () => {
  const { files, source } = await repoTreeFiles("/repo", {
    gitAvailableFn: () => false,
    walkFn: async () => ["x.js"]
  });
  assert.equal(source, "filesystem");
  assert.deepEqual(files, ["x.js"]);
});

test("falls back when git returns nothing or throws", async () => {
  const empty = await repoTreeFiles("/repo", { gitAvailableFn: () => true, gitFn: () => "", walkFn: async () => ["y.js"] });
  assert.equal(empty.source, "filesystem");
  const threw = await repoTreeFiles("/repo", { gitAvailableFn: () => true, gitFn: () => { throw new Error("no git"); }, walkFn: async () => ["z.js"] });
  assert.equal(threw.source, "filesystem");
});

test("the real filesystem walk excludes node_modules/.git/dist and sorts", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "architext-tree-"));
  try {
    await mkdir(path.join(root, "src"), { recursive: true });
    await mkdir(path.join(root, "node_modules", "pkg"), { recursive: true });
    await mkdir(path.join(root, "dist"), { recursive: true });
    await writeFile(path.join(root, "src", "main.ts"), "");
    await writeFile(path.join(root, "README.md"), "");
    await writeFile(path.join(root, "node_modules", "pkg", "index.js"), "");
    await writeFile(path.join(root, "dist", "bundle.js"), "");

    const { files, source } = await repoTreeFiles(root, { gitAvailableFn: () => false });
    assert.equal(source, "filesystem");
    assert.deepEqual(files, ["README.md", "src/main.ts"]);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
