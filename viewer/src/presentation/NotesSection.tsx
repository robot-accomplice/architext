import React, { useState } from "react";
import { notesForTarget } from "../../../src/domain/architecture-model/notes.mjs";
import type { ElementNote, NoteCategory, NoteTargetKind, Id } from "../domain/architectureTypes.js";

const CATEGORIES: { value: NoteCategory; label: string }[] = [
  { value: "note", label: "Note" },
  { value: "mitigation", label: "Mitigation" },
  { value: "caveat", label: "Caveat" },
  { value: "todo", label: "To-do" }
];

function formatWhen(iso: string): string {
  const date = new Date(iso);
  return Number.isNaN(date.getTime()) ? iso : date.toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
}

type Draft = { id: Id | null; category: NoteCategory; body: string };
const EMPTY_DRAFT: Draft = { id: null, category: "note", body: "" };

export function NotesSection({ targetKind, targetId, notes, onSave, onDelete }: {
  targetKind: NoteTargetKind;
  targetId: Id;
  notes: ElementNote[];
  onSave: (note: ElementNote) => Promise<void>;
  onDelete: (id: Id) => Promise<void>;
}) {
  const [draft, setDraft] = useState<Draft | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const mine = notesForTarget(notes ?? [], targetKind, targetId) as ElementNote[];

  const startAdd = () => { setError(null); setDraft({ ...EMPTY_DRAFT }); };
  const startEdit = (note: ElementNote) => { setError(null); setDraft({ id: note.id, category: note.category, body: note.body }); };
  const cancel = () => { setDraft(null); setError(null); };

  const save = async () => {
    if (!draft || !draft.body.trim()) return;
    setBusy(true);
    setError(null);
    const now = new Date().toISOString();
    const existing = draft.id ? mine.find((n) => n.id === draft.id) : null;
    const note: ElementNote = {
      id: draft.id ?? `note-${Date.now().toString(36)}`,
      target: { kind: targetKind, id: targetId },
      category: draft.category,
      body: draft.body.trim(),
      createdAt: existing?.createdAt ?? now,
      updatedAt: now
    };
    try {
      await onSave(note);
      setDraft(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const remove = async (id: Id) => {
    setBusy(true);
    setError(null);
    try {
      await onDelete(id);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="detail-section notes-section" id="notes">
      <div className="notes-head">
        <h3>Notes</h3>
        {!draft && <button type="button" className="notes-add" onClick={startAdd}>+ Add note</button>}
      </div>

      {mine.length === 0 && !draft && <p className="notes-empty">No notes yet.</p>}

      <ul className="notes-list">
        {mine.map((note) => (
          <li key={note.id} className={`note-item cat-${note.category}`}>
            {draft?.id === note.id ? (
              <NoteEditor draft={draft} setDraft={setDraft} onSave={save} onCancel={cancel} busy={busy} />
            ) : (
              <>
                <div className="note-meta">
                  <span className={`note-cat cat-${note.category}`}>{note.category}</span>
                  <span className="note-when">{formatWhen(note.updatedAt)}</span>
                  <span className="note-actions">
                    <button type="button" onClick={() => startEdit(note)} disabled={busy}>Edit</button>
                    <button type="button" onClick={() => remove(note.id)} disabled={busy}>Delete</button>
                  </span>
                </div>
                <p className="note-body">{note.body}</p>
              </>
            )}
          </li>
        ))}
      </ul>

      {draft && draft.id === null && (
        <NoteEditor draft={draft} setDraft={setDraft} onSave={save} onCancel={cancel} busy={busy} />
      )}

      {error && <p className="notes-error">{error}</p>}
    </section>
  );
}

function NoteEditor({ draft, setDraft, onSave, onCancel, busy }: {
  draft: Draft;
  setDraft: (d: Draft) => void;
  onSave: () => void;
  onCancel: () => void;
  busy: boolean;
}) {
  return (
    <div className="note-editor">
      <select value={draft.category} onChange={(e) => setDraft({ ...draft, category: e.target.value as NoteCategory })} disabled={busy}>
        {CATEGORIES.map((c) => <option key={c.value} value={c.value}>{c.label}</option>)}
      </select>
      <textarea
        value={draft.body}
        onChange={(e) => setDraft({ ...draft, body: e.target.value })}
        placeholder="What should a reader know about this element?"
        rows={3}
        disabled={busy}
        autoFocus
      />
      <div className="note-editor-actions">
        <button type="button" className="note-save" onClick={onSave} disabled={busy || !draft.body.trim()}>{busy ? "Saving…" : "Save"}</button>
        <button type="button" onClick={onCancel} disabled={busy}>Cancel</button>
      </div>
    </div>
  );
}
