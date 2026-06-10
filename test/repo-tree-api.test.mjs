import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, mkdir, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { repoTreeFiles } from "../src/adapters/http/repo-tree-api.mjs";

const fakeStat = async () => ({ size: 10, mtimeMs: 1234.6 });

test("uses git ls-files when the target is a git work tree", async () => {
  const { files, source } = await repoTreeFiles("/repo", {
    gitAvailableFn: () => true,
    gitFn: () => "src/a.js\nsrc/b/c.js\nREADME.md\n",
    statFn: fakeStat
  });
  assert.equal(source, "git");
  assert.deepEqual(files, [
    { path: "src/a.js", size: 10, mtime: 1235 },
    { path: "src/b/c.js", size: 10, mtime: 1235 },
    { path: "README.md", size: 10, mtime: 1235 }
  ]);
});

test("falls back to the filesystem walk when not a git repo", async () => {
  const { files, source } = await repoTreeFiles("/repo", {
    gitAvailableFn: () => false,
    walkFn: async () => ["x.js"],
    statFn: fakeStat
  });
  assert.equal(source, "filesystem");
  assert.deepEqual(files, [{ path: "x.js", size: 10, mtime: 1235 }]);
});

test("a file that cannot be stat'd still renders with null metadata", async () => {
  const { files } = await repoTreeFiles("/repo", {
    gitAvailableFn: () => true,
    gitFn: () => "ghost.js\n",
    statFn: async () => { throw new Error("ENOENT"); }
  });
  assert.deepEqual(files, [{ path: "ghost.js", size: null, mtime: null }]);
});

test("falls back when git returns nothing or throws", async () => {
  const empty = await repoTreeFiles("/repo", { gitAvailableFn: () => true, gitFn: () => "", walkFn: async () => ["y.js"], statFn: fakeStat });
  assert.equal(empty.source, "filesystem");
  const threw = await repoTreeFiles("/repo", { gitAvailableFn: () => true, gitFn: () => { throw new Error("no git"); }, walkFn: async () => ["z.js"], statFn: fakeStat });
  assert.equal(threw.source, "filesystem");
});

test("the real filesystem walk excludes node_modules/.git/dist, sorts, and stats", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "architext-tree-"));
  try {
    await mkdir(path.join(root, "src"), { recursive: true });
    await mkdir(path.join(root, "node_modules", "pkg"), { recursive: true });
    await mkdir(path.join(root, "dist"), { recursive: true });
    await writeFile(path.join(root, "src", "main.ts"), "hello");
    await writeFile(path.join(root, "README.md"), "");
    await writeFile(path.join(root, "node_modules", "pkg", "index.js"), "");
    await writeFile(path.join(root, "dist", "bundle.js"), "");

    const { files, source } = await repoTreeFiles(root, { gitAvailableFn: () => false });
    assert.equal(source, "filesystem");
    assert.deepEqual(files.map((f) => f.path), ["README.md", "src/main.ts"]);
    const main = files.find((f) => f.path === "src/main.ts");
    assert.equal(main.size, 5);
    assert.equal(typeof main.mtime, "number");
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
