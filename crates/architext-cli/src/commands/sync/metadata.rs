//! Port of `readMetadata` and `writeMetadata` from `architext-cli.mjs`.

use std::path::Path;

use architext_core::json_write::write_json_string;
use serde_json::{json, Map, Value};

use super::target_layout::{legacy_metadata_path, metadata_path};
use super::timestamp::now_iso;

/// Port of `readMetadata(target)`.
pub fn read_metadata(target: &Path) -> Option<Value> {
    let current = metadata_path(target);
    let legacy = legacy_metadata_path(target);
    if current.exists() {
        let text = std::fs::read_to_string(&current).ok()?;
        return serde_json::from_str(&text).ok();
    }
    if legacy.exists() {
        let text = std::fs::read_to_string(&legacy).ok()?;
        return serde_json::from_str(&text).ok();
    }
    None
}

/// Port of `writeMetadata(target, patch)`.
///
/// Merges `existing` + `patch` under the schema-2 envelope and writes to
/// `.architext.json`.
pub fn write_metadata(target: &Path, patch: &Value) -> std::io::Result<Value> {
    let existing = read_metadata(target);
    let now = now_iso();

    // JS:
    //   const next = {
    //     schemaVersion: 2,
    //     installedAt: existing?.installedAt ?? new Date().toISOString(),
    //     updatedAt: new Date().toISOString(),
    //     ...existing,
    //     ...patch
    //   };
    let installed_at = existing
        .as_ref()
        .and_then(|e| e["installedAt"].as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| now.clone());

    let mut next = Map::new();
    next.insert("schemaVersion".to_string(), json!(2));
    next.insert("installedAt".to_string(), Value::String(installed_at));
    next.insert("updatedAt".to_string(), Value::String(now));

    // Spread existing (overrides the defaults above except for our fixed fields)
    if let Some(Value::Object(existing_obj)) = &existing {
        for (k, v) in existing_obj {
            next.insert(k.clone(), v.clone());
        }
    }

    // Spread patch (wins over existing)
    if let Some(patch_obj) = patch.as_object() {
        for (k, v) in patch_obj {
            next.insert(k.clone(), v.clone());
        }
    }

    // Re-apply the outer envelope fields so they always have correct values
    // (patch might have overwritten installedAt — we want to preserve existing).
    // The JS ordering is: base fields first, then existing spreads over them,
    // then patch spreads over that. So `installedAt` from patch wins over existing.
    // We already did this in order above, but `schemaVersion` must remain 2.
    next.insert("schemaVersion".to_string(), json!(2));

    // The JS spread order means: patch overrides existing overrides defaults.
    // But `installedAt` has a special rule: it comes from existing if present.
    // Since we spread existing AFTER setting installedAt, existing's installedAt
    // would overwrite. Then patch spreads over that. So we need to re-apply the
    // "preserved from existing" logic after the patch spread.
    //
    // Actually re-reading the JS:
    //   { schemaVersion: 2, installedAt: existing?.installedAt ?? now, updatedAt: now,
    //     ...existing,  ← overwrites installedAt/updatedAt with existing's values
    //     ...patch       ← overwrites everything with patch values }
    //
    // So: schemaVersion=2 is ALWAYS 2 (patch can't override it since we apply last).
    // installedAt = patch.installedAt ?? existing.installedAt ?? now
    // updatedAt = patch.updatedAt ?? existing.updatedAt ?? now
    //
    // But wait — the JS puts schemaVersion=2 as the FIRST key, then spreads
    // existing (which may have schemaVersion), then spreads patch. So schemaVersion
    // ends up as whatever patch has or existing has, NOT necessarily 2.
    // But syncMetadataPatch does NOT set schemaVersion in the patch.
    // And if the existing metadata has schemaVersion=1 (legacy), the spread would
    // set it back to 1 — except the top-level schemaVersion:2 is the first key and
    // the spread order means existing's schemaVersion overwrites it.
    //
    // So the actual final value is: last writer wins.
    // Order: {schemaVersion:2} → spread(existing) → spread(patch)
    // existing has schemaVersion=2 (or absent), patch has no schemaVersion.
    // → schemaVersion ends up as existing.schemaVersion ?? 2.
    // For a fresh install there's no existing, so schemaVersion=2. ✓
    // For re-sync with schema=2 existing, it stays 2. ✓
    // We keep our final override of schemaVersion=2 to be safe since
    // patch never sets it.

    let path = metadata_path(target);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let value = Value::Object(next);
    std::fs::write(&path, write_json_string(&value).as_bytes())?;
    Ok(value)
}
