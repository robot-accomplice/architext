import { readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { deleteNote, upsertNote } from "../../domain/architecture-model/notes.mjs";

const DEFAULT_NOTES_FILE = "notes.json";

const notesActionHandlers = {
  delete: (document, payload) => deleteNote(document, payload.id),
  update: (document, payload) => upsertNote(document, payload.note)
};

function applyNotesAction(document, payload) {
  const action = payload.action ?? "update";
  const handler = notesActionHandlers[action];
  if (!handler) throw new Error(`Unknown notes action "${action}"`);
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
        if (snapshot.exists) await writeFile(file, snapshot.content, "utf8");
        else await rm(file, { force: true });
      }
    }
  };
}

async function withoutTargetWriteLock(_target, callback) {
  return callback();
}

export async function updateNotesRequest({
  target,
  payload,
  dataDir,
  readJson,
  writeJson,
  validateTarget,
  withTargetWriteLock = withoutTargetWriteLock
}) {
  return withTargetWriteLock(target, () => updateNotesRequestUnlocked({
    target,
    payload,
    dataDir,
    readJson,
    writeJson,
    validateTarget
  }));
}

async function updateNotesRequestUnlocked({ target, payload, dataDir, readJson, writeJson, validateTarget }) {
  const targetDataDir = dataDir(target);
  const manifestPath = path.join(targetDataDir, "manifest.json");
  const manifest = await readJson(manifestPath);

  // Self-bootstrap: notes are an optional file, so the first write registers
  // manifest.files.notes and creates the file rather than requiring sync.
  const notesRelPath = manifest.files?.notes ?? DEFAULT_NOTES_FILE;
  const manifestNeedsNotesEntry = !manifest.files?.notes;
  const notesPath = path.join(targetDataDir, notesRelPath);

  let notesDocument;
  try {
    notesDocument = await readJson(notesPath);
  } catch (error) {
    if (error?.code !== "ENOENT" && !/ENOENT|no such file/i.test(error?.message ?? "")) throw error;
    notesDocument = { notes: [] };
  }

  const nextDocument = applyNotesAction(notesDocument, payload);
  const writeSet = createWriteSet({ writeJson });
  let validation;
  try {
    await writeSet.writeJson(notesPath, nextDocument);
    if (manifestNeedsNotesEntry) {
      await writeSet.writeJson(manifestPath, {
        ...manifest,
        files: { ...manifest.files, notes: notesRelPath }
      });
    }
    validation = await validateTarget(target);
    if (!validation.ok) {
      throw new Error(`Notes update did not validate:\n${validation.output}`);
    }
  } catch (error) {
    await writeSet.restore();
    throw error;
  }
  return { notes: nextDocument.notes, validation };
}
