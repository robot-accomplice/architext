import { test } from "node:test";
import assert from "node:assert/strict";
import { buildRepoTree, buildOwnerIndex, resolveOwner, dominantOwner } from "../viewer/src/presentation/repoTreeModel.js";

test("buildRepoTree nests paths, dirs before files, alphabetical", () => {
  const root = buildRepoTree(["src/b.js", "src/a.js", "README.md", "src/lib/x.js"]);
  assert.deepEqual(root.children.map((c) => `${c.type}:${c.name}`), ["dir:src", "file:README.md"]);
  const src = root.children.find((c) => c.name === "src");
  assert.deepEqual(src.children.map((c) => `${c.type}:${c.name}`), ["dir:lib", "file:a.js", "file:b.js"]);
});

test("resolveOwner picks the longest matching sourcePath (file beats folder)", () => {
  const nodes = [
    { id: "router", type: "service", sourcePaths: ["src/routing"] },
    { id: "scoring", type: "service", sourcePaths: ["src/routing/routeScoring.js"] }
  ];
  const index = buildOwnerIndex(nodes);
  assert.equal(resolveOwner("src/routing/routeScoring.js", index).id, "scoring"); // exact file wins
  assert.equal(resolveOwner("src/routing/routeEdges.js", index).id, "router");   // folder prefix
  assert.equal(resolveOwner("src/other.js", index), null);                       // unowned
});

test("a sourcePath does not leak across sibling prefixes", () => {
  const index = buildOwnerIndex([{ id: "a", type: "client", sourcePaths: ["src/app"] }]);
  assert.equal(resolveOwner("src/appendix/x.js", index), null); // "src/app" must not match "src/appendix/..."
  assert.equal(resolveOwner("src/app/x.js", index).id, "a");
});

test("dominantOwner reports the majority owner and whether the folder is mixed", () => {
  const root = buildRepoTree(["pkg/a.js", "pkg/b.js", "pkg/c.js"]);
  const index = buildOwnerIndex([
    { id: "svc", type: "service", sourcePaths: ["pkg/a.js", "pkg/b.js"] },
    { id: "data", type: "data-store", sourcePaths: ["pkg/c.js"] }
  ]);
  const pkg = root.children.find((c) => c.name === "pkg");
  const { owner, mixed } = dominantOwner(pkg, index);
  assert.equal(owner.id, "svc");
  assert.equal(mixed, true);
});
