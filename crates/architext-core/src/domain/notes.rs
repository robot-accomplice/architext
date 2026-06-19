//! Pure port of `src/domain/architecture-model/notes.mjs`.

use serde_json::Value;

/// `upsertNote(notesDocument, note)` — insert or merge-update by id.
pub fn upsert_note(notes_document: &Value, note: &Value) -> Result<Value, String> {
    let id = note["id"].as_str().unwrap_or("");
    let notes_arr = notes_document["notes"].as_array()
        .ok_or_else(|| "notes must be an array".to_string())?;

    let existing = notes_arr.iter().find(|c| c["id"].as_str() == Some(id));

    let next_note = if let Some(e) = existing {
        let mut merged = e.clone();
        if let Some(note_obj) = note.as_object() {
            let merged_obj = merged.as_object_mut().unwrap();
            for (k, v) in note_obj {
                merged_obj.insert(k.clone(), v.clone());
            }
        }
        merged
    } else {
        note.clone()
    };

    let new_notes: Vec<Value> = if existing.is_some() {
        notes_arr.iter().map(|c| {
            if c["id"].as_str() == Some(id) { next_note.clone() } else { c.clone() }
        }).collect()
    } else {
        let mut v = notes_arr.clone();
        v.push(next_note);
        v
    };

    let mut out = notes_document.clone();
    out.as_object_mut().unwrap().insert("notes".to_string(), Value::Array(new_notes));
    Ok(out)
}

/// `deleteNote(notesDocument, id)`.
pub fn delete_note(notes_document: &Value, id: &str) -> Result<Value, String> {
    let notes_arr = notes_document["notes"].as_array()
        .ok_or_else(|| "notes must be an array".to_string())?;

    let existing = notes_arr.iter().find(|c| c["id"].as_str() == Some(id));
    if existing.is_none() {
        return Err(format!("Note \"{id}\" was not found"));
    }

    let new_notes: Vec<Value> = notes_arr.iter()
        .filter(|c| c["id"].as_str() != Some(id))
        .cloned()
        .collect();

    let mut out = notes_document.clone();
    out.as_object_mut().unwrap().insert("notes".to_string(), Value::Array(new_notes));
    Ok(out)
}

/// `notesForTarget(notes, kind, id)` — filter by target.kind+id, sort newest-first.
///
/// JS: `.sort((a, b) => String(b.updatedAt ?? "").localeCompare(String(a.updatedAt ?? "")))`
/// For ISO date strings, localeCompare on ASCII chars is byte order = lexicographic desc.
pub fn notes_for_target(notes: &[Value], kind: &str, id: &str) -> Vec<Value> {
    let mut filtered: Vec<Value> = notes.iter()
        .filter(|n| {
            n["target"]["kind"].as_str() == Some(kind)
                && n["target"]["id"].as_str() == Some(id)
        })
        .cloned()
        .collect();
    // Stable sort, newest-first: String(b.updatedAt ?? "").localeCompare(String(a.updatedAt ?? ""))
    // For ASCII date strings this is reverse-lexicographic.
    filtered.sort_by(|a, b| {
        let ba = string_updated_at(b);
        let aa = string_updated_at(a);
        // localeCompare returns b compared to a → descending
        ba.cmp(&aa)
    });
    filtered
}

/// Mirrors JS `String(note.updatedAt ?? "")`.
fn string_updated_at(note: &Value) -> String {
    let v = &note["updatedAt"];
    if v.is_null() {
        return String::new();
    }
    // Field missing → Value::Null from index access
    v.as_str().map(|s| s.to_string()).unwrap_or_else(|| v.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn note(id: &str, target_kind: &str, target_id: &str, updated_at: &str) -> Value {
        json!({
            "id": id,
            "target": { "kind": target_kind, "id": target_id },
            "updatedAt": updated_at,
            "text": "some text"
        })
    }

    #[test]
    fn upsert_inserts_new_note() {
        let doc = json!({ "notes": [] });
        let n = note("n1", "node", "node-1", "2024-01-01");
        let out = upsert_note(&doc, &n).unwrap();
        assert_eq!(out["notes"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn upsert_updates_existing_note() {
        let doc = json!({ "notes": [note("n1", "node", "node-1", "2024-01-01")] });
        let updated = json!({ "id": "n1", "text": "updated", "target": {"kind":"node","id":"node-1"}, "updatedAt": "2024-06-01" });
        let out = upsert_note(&doc, &updated).unwrap();
        assert_eq!(out["notes"][0]["text"], "updated");
        assert_eq!(out["notes"][0]["updatedAt"], "2024-06-01");
    }

    #[test]
    fn delete_note_removes_it() {
        let doc = json!({ "notes": [note("n1", "node", "node-1", "2024-01-01")] });
        let out = delete_note(&doc, "n1").unwrap();
        assert!(out["notes"].as_array().unwrap().is_empty());
    }

    #[test]
    fn delete_note_not_found_error() {
        let doc = json!({ "notes": [] });
        let err = delete_note(&doc, "missing").unwrap_err();
        assert_eq!(err, "Note \"missing\" was not found");
    }

    #[test]
    fn notes_for_target_filters_and_sorts_newest_first() {
        let notes = vec![
            note("n1", "node", "node-1", "2024-01-01"),
            note("n2", "node", "node-1", "2024-06-01"),
            note("n3", "node", "node-2", "2024-03-01"),
        ];
        let result = notes_for_target(&notes, "node", "node-1");
        assert_eq!(result.len(), 2);
        // n2 is newer
        assert_eq!(result[0]["id"], "n2");
        assert_eq!(result[1]["id"], "n1");
    }

    #[test]
    fn notes_for_target_empty_updated_at() {
        let notes = vec![
            json!({ "id": "n1", "target": { "kind": "node", "id": "x" }, "text": "t" }),
            note("n2", "node", "x", "2024-01-01"),
        ];
        let result = notes_for_target(&notes, "node", "x");
        // n2 (has date) sorts before n1 (empty string)
        assert_eq!(result[0]["id"], "n2");
        assert_eq!(result[1]["id"], "n1");
    }
}
