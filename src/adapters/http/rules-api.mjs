import path from "node:path";
import { deleteRule, moveRule, moveRuleBefore, upsertRule } from "../../domain/architecture-model/rules.mjs";

export async function updateRulesRequest({
  target,
  payload,
  dataDir,
  readJson,
  writeJson,
  validateTarget
}) {
  const targetDataDir = dataDir(target);
  const manifest = await readJson(path.join(targetDataDir, "manifest.json"));
  if (!manifest.files?.rules) throw new Error("Rules editing requires manifest.files.rules");
  const rulesPath = path.join(targetDataDir, manifest.files.rules);
  const rulesDocument = await readJson(rulesPath);
  const action = payload.action ?? "update";
  const nextDocument = action === "delete"
    ? deleteRule(rulesDocument, payload.id)
    : action === "move"
      ? moveRule(rulesDocument, payload.id, payload.direction)
      : action === "move-before"
        ? moveRuleBefore(rulesDocument, payload.id, payload.beforeId)
      : action === "update"
        ? upsertRule(rulesDocument, payload.rule)
        : null;
  if (!nextDocument) throw new Error(`Unknown rules action "${action}"`);
  await writeJson(rulesPath, nextDocument);
  const validation = await validateTarget(target);
  if (!validation.ok) {
    await writeJson(rulesPath, rulesDocument);
    throw new Error(`Rules update did not validate:\n${validation.output}`);
  }
  return { rules: nextDocument.rules, validation };
}
