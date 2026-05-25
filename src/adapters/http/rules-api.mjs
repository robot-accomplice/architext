import path from "node:path";
import { deleteRule, moveRule, moveRuleBefore, upsertRule } from "../../domain/architecture-model/rules.mjs";

const rulesActionHandlers = {
  delete: (document, payload) => deleteRule(document, payload.id),
  move: (document, payload) => moveRule(document, payload.id, payload.direction),
  "move-before": (document, payload) => moveRuleBefore(document, payload.id, payload.beforeId),
  update: (document, payload) => upsertRule(document, payload.rule)
};

function applyRulesAction(document, payload) {
  const action = payload.action ?? "update";
  const handler = rulesActionHandlers[action];
  if (!handler) throw new Error(`Unknown rules action "${action}"`);
  return handler(document, payload);
}

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
  const nextDocument = applyRulesAction(rulesDocument, payload);
  await writeJson(rulesPath, nextDocument);
  const validation = await validateTarget(target);
  if (!validation.ok) {
    await writeJson(rulesPath, rulesDocument);
    throw new Error(`Rules update did not validate:\n${validation.output}`);
  }
  return { rules: nextDocument.rules, validation };
}
