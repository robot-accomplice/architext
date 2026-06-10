// Loads the resolved diagram config for `architext serve` and the `/api/config`
// endpoint. Reads the user-global config (~/.architext/config.json) and the
// project config (docs/architext/config.json), then resolves them against the
// built-in defaults. Missing files are normal; unreadable or malformed files
// degrade to a warning and fall through — serving never fails on bad config.

import path from "node:path";
import os from "node:os";
import { readFile as fsReadFile, mkdir, writeFile } from "node:fs/promises";
import { architextDir } from "../../domain/lifecycle/target-layout.mjs";
import {
  DIAGRAM_CONFIG_FIELDS,
  SECTION_LABELS,
  diffDiagramConfigFromDefaults,
  resolveDiagramConfig
} from "../../domain/diagram-config/diagram-config.mjs";

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

// GET /api/config payload: the resolved config plus the field/section spec that
// drives the config UI (single source of truth — the viewer renders controls
// from this rather than duplicating ranges/labels).
export async function diagramConfigGetPayload(target, { homedir = os.homedir() } = {}) {
  const { config, warnings } = await loadDiagramConfig(target, { homedir });
  return { diagram: config, warnings, fields: DIAGRAM_CONFIG_FIELDS, sections: SECTION_LABELS };
}

function diagramConfigPathForScope(scope, target, homedir) {
  if (scope === "user") return userDiagramConfigPath(homedir);
  if (scope === "project") return projectDiagramConfigPath(target);
  throw new Error(`Unknown diagram config scope "${scope}" (expected "project" or "user").`);
}

// POST /api/config: persist the supplied (effective) config to the chosen
// layer, writing only the fields that differ from defaults so the file stays
// minimal and precedence stays meaningful. Returns the re-resolved config.
export async function writeDiagramConfig({ scope, target, diagram, homedir = os.homedir(), writeFileFn = writeFile, mkdirFn = mkdir }) {
  const file = diagramConfigPathForScope(scope, target, homedir);
  // Normalize/clamp the incoming values, then reduce to non-default overrides.
  const { config: normalized } = resolveDiagramConfig([{ raw: diagram, source: `${scope} config` }]);
  const overrides = diffDiagramConfigFromDefaults(normalized);
  await mkdirFn(path.dirname(file), { recursive: true });
  await writeFileFn(file, `${JSON.stringify(overrides, null, 2)}\n`, "utf8");
  const resolved = await loadDiagramConfig(target, { homedir });
  return { ok: true, scope, file, written: overrides, diagram: resolved.config, warnings: resolved.warnings };
}
