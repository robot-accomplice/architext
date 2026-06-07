import { test } from "node:test";
import assert from "node:assert/strict";
import {
  resolveDiagramConfigFromFiles,
  userDiagramConfigPath,
  projectDiagramConfigPath
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
