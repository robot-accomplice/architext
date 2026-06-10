// Pure mutations for element notes — user annotations attached to an
// architecture element (node/flow/decision/risk/view/data-class) and persisted
// in notes.json. Mirrors the rules document shape: { notes: [...] }.

export function upsertNote(notesDocument, note) {
  const existing = notesDocument.notes.find((candidate) => candidate.id === note.id);
  const nextNote = existing ? { ...existing, ...note } : note;
  return {
    ...notesDocument,
    notes: existing
      ? notesDocument.notes.map((candidate) => (candidate.id === note.id ? nextNote : candidate))
      : [...notesDocument.notes, nextNote]
  };
}

export function deleteNote(notesDocument, id) {
  const existing = notesDocument.notes.find((candidate) => candidate.id === id);
  if (!existing) throw new Error(`Note "${id}" was not found`);
  return {
    ...notesDocument,
    notes: notesDocument.notes.filter((candidate) => candidate.id !== id)
  };
}

// Notes attached to one element, in stable newest-first order.
export function notesForTarget(notes = [], kind, id) {
  return notes
    .filter((note) => note.target?.kind === kind && note.target?.id === id)
    .sort((a, b) => String(b.updatedAt ?? "").localeCompare(String(a.updatedAt ?? "")));
}
