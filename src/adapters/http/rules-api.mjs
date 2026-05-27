import { readFile, rm, writeFile } from "node:fs/promises";
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

function createWriteSet({ writeJson }) {
  const snapshots = new Map();

  async function capture(file) {
    if (snapshots.has(file)) return;
    try {
      snapshots.set(file, { exists: true, content: await readFile(file, "utf8") });
    } catch (error) {
      if (error?.code !== "ENOENT") throw error;
      snapshots.set(file, { exists: false });
    }
  }

  return {
    async writeJson(file, value) {
      await capture(file);
      await writeJson(file, value);
    },
    async restore() {
      for (const [file, snapshot] of [...snapshots.entries()].reverse()) {
        if (snapshot.exists) {
          await writeFile(file, snapshot.content, "utf8");
        } else {
          await rm(file, { force: true });
        }
      }
    }
  };
}

async function withoutTargetWriteLock(_target, callback) {
  return callback();
}

export async function updateRulesRequest({
  target,
  payload,
  dataDir,
  readJson,
  writeJson,
  validateTarget,
  withTargetWriteLock = withoutTargetWriteLock
}) {
  return withTargetWriteLock(target, () => updateRulesRequestUnlocked({
    target,
    payload,
    dataDir,
    readJson,
    writeJson,
    validateTarget
  }));
}

async function updateRulesRequestUnlocked({
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
  const writeSet = createWriteSet({ writeJson });
  let validation;
  try {
    await writeSet.writeJson(rulesPath, nextDocument);
    validation = await validateTarget(target);
    if (!validation.ok) {
      throw new Error(`Rules update did not validate:\n${validation.output}`);
    }
  } catch (error) {
    await writeSet.restore();
    throw error;
  }
  return { rules: nextDocument.rules, validation };
}
