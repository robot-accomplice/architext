// Loads the resolved diagram config for `architext serve` and the `/api/config`
// endpoint. Reads the user-global config (~/.architext/config.json) and the
// project config (docs/architext/config.json), then resolves them against the
// built-in defaults. Missing files are normal; unreadable or malformed files
// degrade to a warning and fall through — serving never fails on bad config.

import path from "node:path";
import os from "node:os";
import { readFile as fsReadFile } from "node:fs/promises";
import { architextDir } from "../../domain/lifecycle/target-layout.mjs";
import { resolveDiagramConfig } from "../../domain/diagram-config/diagram-config.mjs";

export function userDiagramConfigPath(homedir = os.homedir()) {
  return path.join(homedir, ".architext", "config.json");
}

export function projectDiagramConfigPath(target) {
  return path.join(architextDir(target), "config.json");
}

async function readJsonLayer(file, source, readFileFn, warnings) {
  let text;
  try {
    text = await readFileFn(file, "utf8");
  } catch (error) {
    if (error?.code === "ENOENT") return undefined; // absent config is expected
    warnings.push(`${source}: could not read ${file} (${error.message}); ignored.`);
    return undefined;
  }
  try {
    return JSON.parse(text);
  } catch (error) {
    warnings.push(`${source}: ${file} is not valid JSON (${error.message}); ignored.`);
    return undefined;
  }
}

export async function resolveDiagramConfigFromFiles({ userConfigPath, projectConfigPath, readFileFn = fsReadFile }) {
  const readWarnings = [];
  const userRaw = await readJsonLayer(userConfigPath, "user config", readFileFn, readWarnings);
  const projectRaw = await readJsonLayer(projectConfigPath, "project config", readFileFn, readWarnings);
  const { config, warnings } = resolveDiagramConfig([
    { raw: userRaw, source: "user config" },
    { raw: projectRaw, source: "project config" }
  ]);
  return { config, warnings: [...readWarnings, ...warnings] };
}

export async function loadDiagramConfig(target, { homedir = os.homedir() } = {}) {
  return resolveDiagramConfigFromFiles({
    userConfigPath: userDiagramConfigPath(homedir),
    projectConfigPath: projectDiagramConfigPath(target)
  });
}
