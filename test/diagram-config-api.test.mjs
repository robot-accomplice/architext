import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, rm, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import {
  resolveDiagramConfigFromFiles,
  userDiagramConfigPath,
  projectDiagramConfigPath,
  writeDiagramConfig
} from "../src/adapters/http/diagram-config-api.mjs";

function fakeReader(files) {
  // files: { [path]: string | Error }. Missing path => ENOENT.
  return async (file) => {
    if (!(file in files)) {
      const error = new Error("ENOENT");
      error.code = "ENOENT";
      throw error;
    }
    const value = files[file];
    if (value instanceof Error) throw value;
    return value;
  };
}

test("paths resolve under ~/.architext and docs/architext", () => {
  assert.equal(userDiagramConfigPath("/home/me"), "/home/me/.architext/config.json");
  assert.equal(projectDiagramConfigPath("/repo"), "/repo/docs/architext/config.json");
});

test("absent config files yield defaults with no warnings", async () => {
  const { config, warnings } = await resolveDiagramConfigFromFiles({
    userConfigPath: "/u/config.json",
    projectConfigPath: "/p/config.json",
    readFileFn: fakeReader({})
  });
  assert.equal(config.layout.laneWidth, 210);
  assert.equal(warnings.length, 0);
});

test("project config overrides user config which overrides defaults", async () => {
  const { config } = await resolveDiagramConfigFromFiles({
    userConfigPath: "/u/config.json",
    projectConfigPath: "/p/config.json",
    readFileFn: fakeReader({
      "/u/config.json": JSON.stringify({ layout: { laneWidth: 300, rowGap: 150 } }),
      "/p/config.json": JSON.stringify({ layout: { laneWidth: 400 } })
    })
  });
  assert.equal(config.layout.laneWidth, 400);
  assert.equal(config.layout.rowGap, 150);
  assert.equal(config.layout.nodeWidth, 136);
});

test("malformed JSON degrades to a warning and falls through (never throws)", async () => {
  const { config, warnings } = await resolveDiagramConfigFromFiles({
    userConfigPath: "/u/config.json",
    projectConfigPath: "/p/config.json",
    readFileFn: fakeReader({
      "/u/config.json": "{ this is not json ",
      "/p/config.json": JSON.stringify({ layout: { laneWidth: 333 } })
    })
  });
  assert.equal(config.layout.laneWidth, 333); // project still applies
  assert.ok(warnings.some((w) => /not valid JSON/.test(w)));
});

test("writeDiagramConfig persists only non-default overrides and re-resolves", async () => {
  const target = await mkdtemp(path.join(tmpdir(), "ax-cfg-"));
  const home = await mkdtemp(path.join(tmpdir(), "ax-home-"));
  try {
    const result = await writeDiagramConfig({
      scope: "project",
      target,
      homedir: home,
      diagram: { layout: { laneWidth: 300, nodeWidth: 136 }, zoom: { minFitZoom: 0.15, maxFitZoom: 1.6 } }
    });
    assert.equal(result.ok, true);
    assert.equal(result.scope, "project");
    // Only the field that differs from default is written.
    const onDisk = JSON.parse(await readFile(projectDiagramConfigPath(target), "utf8"));
    assert.deepEqual(onDisk, { layout: { laneWidth: 300 } });
    // Re-resolved effective config reflects the save (empty home => no user layer).
    assert.equal(result.diagram.layout.laneWidth, 300);
    assert.equal(result.diagram.layout.nodeWidth, 136);
  } finally {
    await rm(target, { recursive: true, force: true });
    await rm(home, { recursive: true, force: true });
  }
});

test("writeDiagramConfig clamps out-of-range values before persisting", async () => {
  const target = await mkdtemp(path.join(tmpdir(), "ax-cfg-"));
  const home = await mkdtemp(path.join(tmpdir(), "ax-home-"));
  try {
    await writeDiagramConfig({ scope: "user", target, homedir: home, diagram: { layout: { laneWidth: 99999 } } });
    const onDisk = JSON.parse(await readFile(userDiagramConfigPath(home), "utf8"));
    assert.equal(onDisk.layout.laneWidth, 800); // clamped to max
  } finally {
    await rm(target, { recursive: true, force: true });
    await rm(home, { recursive: true, force: true });
  }
});

test("writeDiagramConfig rejects an unknown scope", async () => {
  await assert.rejects(
    () => writeDiagramConfig({ scope: "global", target: "/x", diagram: {} }),
    /Unknown diagram config scope/
  );
});

test("an unreadable file (non-ENOENT) warns rather than crashing", async () => {
  const eacces = new Error("EACCES");
  eacces.code = "EACCES";
  const { config, warnings } = await resolveDiagramConfigFromFiles({
    userConfigPath: "/u/config.json",
    projectConfigPath: "/p/config.json",
    readFileFn: fakeReader({ "/u/config.json": eacces })
  });
  assert.equal(config.layout.laneWidth, 210);
  assert.ok(warnings.some((w) => /could not read/.test(w)));
});
