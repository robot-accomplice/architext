import { test } from "node:test";
import assert from "node:assert/strict";
import { upsertNote, deleteNote, notesForTarget } from "../src/domain/architecture-model/notes.mjs";

const doc = () => ({
  notes: [
    { id: "note-a", target: { kind: "node", id: "auth" }, category: "mitigation", body: "Rotated", createdAt: "2026-01-01", updatedAt: "2026-01-02" },
    { id: "note-b", target: { kind: "risk", id: "r1" }, category: "note", body: "Watch", createdAt: "2026-01-01", updatedAt: "2026-01-03" }
  ]
});

test("upsertNote adds a new note and updates an existing one in place", () => {
  const added = upsertNote(doc(), { id: "note-c", target: { kind: "node", id: "cache" }, category: "todo", body: "Add TTL", createdAt: "x", updatedAt: "x" });
  assert.equal(added.notes.length, 3);
  const edited = upsertNote(doc(), { id: "note-a", body: "Rotated quarterly", updatedAt: "2026-02-01" });
  assert.equal(edited.notes.length, 2);
  const a = edited.notes.find((n) => n.id === "note-a");
  assert.equal(a.body, "Rotated quarterly");
  assert.equal(a.category, "mitigation"); // preserved from existing
});

test("deleteNote removes by id and throws on unknown", () => {
  const after = deleteNote(doc(), "note-a");
  assert.deepEqual(after.notes.map((n) => n.id), ["note-b"]);
  assert.throws(() => deleteNote(doc(), "missing"), /was not found/);
});

test("notesForTarget filters by element and sorts newest-first", () => {
  const d = doc();
  d.notes.push({ id: "note-d", target: { kind: "node", id: "auth" }, category: "note", body: "Later", createdAt: "x", updatedAt: "2026-05-01" });
  const forAuth = notesForTarget(d.notes, "node", "auth");
  assert.deepEqual(forAuth.map((n) => n.id), ["note-d", "note-a"]); // newest updatedAt first
  assert.deepEqual(notesForTarget(d.notes, "node", "nope"), []);
});
