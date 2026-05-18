import assert from "node:assert/strict";
import path from "node:path";
import test from "node:test";
import {
  architextDir,
  copiedInstallCandidatePaths,
  dataDir,
  generatedIgnores,
  instructionFiles,
  legacyMetadataPath,
  metadataPath,
  rootScripts
} from "../src/domain/lifecycle/target-layout.mjs";

test("target layout centralizes data-only repository paths", () => {
  const target = path.join("tmp", "repo");

  assert.equal(architextDir(target), path.join(target, "docs", "architext"));
  assert.equal(dataDir(target), path.join(target, "docs", "architext", "data"));
  assert.equal(metadataPath(target), path.join(target, "docs", "architext", ".architext.json"));
  assert.equal(legacyMetadataPath(target), path.join(target, "docs", "architext", ".architext-install.json"));
});

test("target layout exposes lifecycle-managed repository conventions", () => {
  assert.deepEqual(instructionFiles, ["AGENTS.md", "CLAUDE.md"]);
  assert.deepEqual(generatedIgnores, ["docs/architext/dist/"]);
  assert.equal(rootScripts.architext, "architext serve .");
  assert.equal(rootScripts["architext:validate"], "architext validate .");
});

test("target layout lists copied install candidates without filesystem policy", () => {
  const candidates = copiedInstallCandidatePaths("/repo");

  assert.ok(candidates.includes(path.join("/repo", "docs", "architext", "src")));
  assert.ok(candidates.includes(path.join("/repo", "docs", "architext", "schema")));
  assert.ok(candidates.includes(path.join("/repo", "docs", "architext", "package.json")));
});
