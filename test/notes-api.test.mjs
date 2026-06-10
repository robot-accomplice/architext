import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, rm, readFile, writeFile, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { updateNotesRequest } from "../src/adapters/http/notes-api.mjs";

const readJson = async (file) => JSON.parse(await readFile(file, "utf8"));
const writeJson = async (file, value) => writeFile(file, JSON.stringify(value, null, 2) + "\n", "utf8");
const exists = async (file) => Boolean(await stat(file).catch(() => null));

async function makeTarget() {
  const dir = await mkdtemp(path.join(tmpdir(), "architext-notes-"));
  await writeJson(path.join(dir, "manifest.json"), { files: { nodes: "nodes.json" }, notes: [] });
  return dir;
}

const deps = (dir, validateTarget) => ({
  target: dir, dataDir: (t) => t, readJson, writeJson, validateTarget
});

test("first upsert self-bootstraps notes.json and registers manifest.files.notes", async () => {
  const dir = await makeTarget();
  try {
    const note = { id: "note-a", target: { kind: "node", id: "auth" }, category: "mitigation", body: "Rotated", createdAt: "x", updatedAt: "x" };
    const result = await updateNotesRequest({ ...deps(dir, async () => ({ ok: true })), payload: { action: "update", note } });
    assert.equal(result.notes.length, 1);
    assert.equal(await exists(path.join(dir, "notes.json")), true);
    const manifest = await readJson(path.join(dir, "manifest.json"));
    assert.equal(manifest.files.notes, "notes.json"); // registered
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test("a note that fails validation is rolled back (file + manifest restored)", async () => {
  const dir = await makeTarget();
  try {
    const note = { id: "note-bad", target: { kind: "node", id: "ghost" }, category: "note", body: "x", createdAt: "x", updatedAt: "x" };
    await assert.rejects(
      updateNotesRequest({ ...deps(dir, async () => ({ ok: false, output: "note note-bad.target references unknown id" })), payload: { action: "update", note } }),
      /did not validate/
    );
    // bootstrap was rolled back: no notes.json, manifest has no files.notes
    assert.equal(await exists(path.join(dir, "notes.json")), false);
    const manifest = await readJson(path.join(dir, "manifest.json"));
    assert.equal(manifest.files.notes, undefined);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test("delete removes a note from an existing notes.json", async () => {
  const dir = await makeTarget();
  try {
    await writeJson(path.join(dir, "notes.json"), { notes: [{ id: "note-x", target: { kind: "node", id: "auth" }, category: "note", body: "y", createdAt: "x", updatedAt: "x" }] });
    await writeJson(path.join(dir, "manifest.json"), { files: { nodes: "nodes.json", notes: "notes.json" }, notes: [] });
    const result = await updateNotesRequest({ ...deps(dir, async () => ({ ok: true })), payload: { action: "delete", id: "note-x" } });
    assert.deepEqual(result.notes, []);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});
