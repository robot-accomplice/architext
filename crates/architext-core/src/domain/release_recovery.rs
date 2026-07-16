//! Release Truth enumeration, recovery, and repair planning (I/O).
//!
//! Extracted from `doctor_repairs.rs` (swarm T-2): this is release-domain
//! logic shared by the status side (`status::generated_release_history_changes`)
//! and the apply side (`doctor_repairs::repair_release_truth_data`), so both
//! compute the identical `ReleaseTruthPlan` and advertised repairs always
//! match applied ones. New Rust-side behavior with no JS predecessor.
//!
//! Recovery contract (maintainer directive E-1): every file a repair
//! overwrites is first backed up with a timestamped `.bak` name; imperfect
//! index-named details are best-effort unmarshalled, backfilled, and
//! re-marshalled schema-valid; each recovery is recorded in
//! `docs/architext/data/repair-advice.json` because reconciling old (backup)
//! vs new (recovered) content is the maintaining agent's responsibility.

use std::path::Path;

use serde_json::{json, Value};

use crate::domain::release;
use crate::json_write::{read_json, write_json};

fn now_secs() -> u64 {
    // Use std time only; no chrono dep needed for a timestamp string
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub(crate) fn chrono_now() -> String {
    // Format as ISO 8601 — approximate (no sub-second, no tz offset awareness)
    let (y, mo, d, h, mi, sec) = secs_to_datetime(now_secs());
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{sec:02}.000Z")
}

fn secs_to_datetime(secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let sec = secs % 60;
    let mins = secs / 60;
    let min = mins % 60;
    let hours = mins / 60;
    let hour = hours % 24;
    let days = hours / 24;

    // Simplified Gregorian calendar
    let mut year = 1970u64;
    let mut remaining = days;
    loop {
        let leap = is_leap(year);
        let days_in_year = if leap { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        year += 1;
    }
    let leap = is_leap(year);
    let month_days: [u64; 12] = if leap {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 0u64;
    for days_in_month in &month_days {
        if remaining < *days_in_month {
            break;
        }
        remaining -= days_in_month;
        month += 1;
    }
    (year, month + 1, remaining + 1, hour, min, sec)
}

fn is_leap(year: u64) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

// ─── release detail enumeration + repair plan ────────────────────────────────

/// Discover release detail entries directly from the releases directory, for the
/// case where no readable index exists to enumerate them. (New Rust-side behavior
/// closing an apply-side gap — the JS `applyDoctorRepairs` had no equivalent and
/// skipped this repair.) A `*.json` file counts as a detail only if it parses and
/// carries every summary-source field (`id`, `version`, `name`, `status`,
/// `posture`, `summary`, `lastUpdated`) as a non-blank string — the scanned
/// directory can hold unrelated or half-written JSON (manifest.json when the
/// index lives in the data root, tool or editor artifacts, partial stubs), which
/// must not be swept into the regenerated index as null-field entries. The check
/// is deliberately presence-only, not enum/pattern validation: a detail with an
/// invalid status or id is malformed release HISTORY that post-repair validation
/// must flag loudly, not data for this scan to silently drop. The index file is
/// excluded by name; unreadable files are skipped. Sorted by file name for
/// deterministic output.
fn release_detail_entries_from_dir(release_dir: &Path, index_path: &Path) -> Value {
    let index_file_name = index_path.file_name();
    let mut files: Vec<String> = std::fs::read_dir(release_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.file_name() == index_file_name
                || path.extension().and_then(|e| e.to_str()) != Some("json")
            {
                return None;
            }
            path.file_name().and_then(|n| n.to_str()).map(|n| n.to_string())
        })
        .collect();
    files.sort();
    let entries: Vec<Value> = files
        .iter()
        .filter_map(|file| {
            let detail = read_json(&release_dir.join(file))?;
            if !is_complete_detail(&detail) {
                return None;
            }
            Some(json!({ "file": file, "detail": detail }))
        })
        .collect();
    Value::Array(entries)
}

/// The summary-source fields every release detail must carry as non-blank
/// strings for the generated index entry to be schema-valid.
const DETAIL_SUMMARY_FIELDS: [&str; 7] =
    ["id", "version", "name", "status", "posture", "summary", "lastUpdated"];

fn is_complete_detail(detail: &Value) -> bool {
    DETAIL_SUMMARY_FIELDS
        .iter()
        .all(|field| detail[*field].as_str().is_some_and(|s| !s.trim().is_empty()))
}

/// Best-effort recovery of an index-named release detail (E-1 human directive):
/// keep everything the original carries, backfill missing/blank summary-source
/// fields from the index summary, then from explicit defaults, and ensure the
/// schema-required containers exist — so the re-marshalled detail (and the
/// index generated from it) is always schema-valid. Returns the recovered
/// detail plus the list of fields that were backfilled, which the repair
/// records as reconciliation advice for the maintaining agent.
fn normalize_release_detail(
    detail: Option<&Value>,
    summary: &Value,
    now: &str,
) -> (Value, Vec<String>) {
    let mut backfilled: Vec<String> = Vec::new();
    let mut obj = detail
        .and_then(|d| d.as_object().cloned())
        .unwrap_or_default();

    let recovered_id = obj
        .get("id")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .or_else(|| summary["id"].as_str().filter(|s| !s.trim().is_empty()))
        .unwrap_or("recovered-release")
        .to_string();

    let defaults: [(&str, String); 7] = [
        ("id", recovered_id.clone()),
        ("version", "0.0.0".to_string()),
        ("name", recovered_id.clone()),
        ("status", "draft".to_string()),
        ("posture", "at-risk".to_string()),
        (
            "summary",
            "Recovered by architext doctor from an incomplete release detail; review and complete."
                .to_string(),
        ),
        ("lastUpdated", now.to_string()),
    ];
    for (field, default) in defaults {
        let blank = obj
            .get(field)
            .and_then(Value::as_str)
            .map(|s| s.trim().is_empty())
            .unwrap_or(true);
        if blank {
            let value = summary[field]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.to_string())
                .unwrap_or(default);
            obj.insert(field.to_string(), Value::String(value));
            backfilled.push(field.to_string());
        }
    }
    for optional in ["releasedAt", "targetDate", "targetWindow"] {
        if !obj.contains_key(optional) {
            if let Some(s) = summary[optional].as_str() {
                obj.insert(optional.to_string(), Value::String(s.to_string()));
                backfilled.push(optional.to_string());
            }
        }
    }
    // Validation requires a target for draft/planned/implementing releases. A
    // placeholder window is an honest plan statement demanding review; a
    // fabricated releasedAt for a completed release would be invented history,
    // so that case is deliberately left for validation to flag.
    let status = obj.get("status").and_then(Value::as_str).unwrap_or("");
    if matches!(status, "draft" | "planned" | "implementing")
        && !obj.contains_key("targetDate")
        && !obj.contains_key("targetWindow")
    {
        obj.insert(
            "targetWindow".to_string(),
            Value::String("TBD — set a target window (recovered detail)".to_string()),
        );
        backfilled.push("targetWindow".to_string());
    }
    if !obj.get("scope").map(Value::is_object).unwrap_or(false) {
        obj.insert("scope".to_string(), json!({}));
    }
    if let Some(scope) = obj.get_mut("scope").and_then(Value::as_object_mut) {
        for section in ["required", "planned", "stretch", "deferred", "outOfScope"] {
            if !scope.get(section).map(Value::is_array).unwrap_or(false) {
                scope.insert(section.to_string(), json!([]));
            }
        }
    }
    for container in ["workstreams", "blockers", "milestones", "dependencies", "evidence"] {
        if !obj.get(container).map(Value::is_array).unwrap_or(false) {
            obj.insert(container.to_string(), json!([]));
            backfilled.push(container.to_string());
        }
    }
    (Value::Object(obj), backfilled)
}

/// Standing instruction recorded with every recovery: the repair produces a
/// mechanically valid file, but only the maintaining agent can restore the
/// real facts.
pub(crate) const RECONCILE_INSTRUCTION: &str = "architext doctor recovered this file with \
backfilled placeholder values. Reconciling recovered content is YOUR \
responsibility: restore real facts from the timestamped backup, the source \
code, and git history; replace every placeholder; run `architext validate`; \
then delete the backup file and remove this advice entry.";

/// Append recovery entries to `docs/architext/data/repair-advice.json`,
/// creating it if absent and preserving unresolved entries from earlier runs.
/// A ledger that exists but does not parse as expected is backed up before the
/// file is reset — and never clobbered if that backup fails. A new recovery of
/// a file supersedes any earlier entry for the same file (whose backup
/// reference may already be stale). The maintaining agent removes entries as
/// it reconciles them.
pub(crate) fn append_repair_advice(target_data_dir: &Path, new_entries: Vec<Value>) {
    let advice_path = target_data_dir.join("repair-advice.json");
    let existing = read_json(&advice_path).filter(|v| v["advice"].is_array());
    if existing.is_none() && advice_path.exists() && backup_file(&advice_path).is_none() {
        return;
    }
    let mut advice = existing.unwrap_or_else(|| json!({ "advice": [] }));
    let new_files: std::collections::HashSet<String> = new_entries
        .iter()
        .filter_map(|e| e["file"].as_str().map(|s| s.to_string()))
        .collect();
    if let Some(list) = advice["advice"].as_array_mut() {
        list.retain(|e| {
            e["file"].as_str().map(|f| !new_files.contains(f)).unwrap_or(true)
        });
        list.extend(new_entries);
    }
    let _ = write_json(&advice_path, &advice);
}

/// Copy `path` to a timestamped sibling (`<name>.<yyyymmddThhmmssZ>.bak`)
/// before it is overwritten. The `.bak` extension keeps backups out of the
/// `*.json` dir scan. An existing backup is never clobbered: a `-N` suffix is
/// appended until the name is free (same-second repairs would otherwise
/// overwrite the earlier backup — the artifact this exists to preserve).
/// Returns the backup file name on success.
pub(crate) fn backup_file(path: &Path) -> Option<String> {
    let (y, mo, d, h, mi, s) = secs_to_datetime(now_secs());
    let base = format!(
        "{}.{y:04}{mo:02}{d:02}T{h:02}{mi:02}{s:02}Z",
        path.file_name()?.to_str()?
    );
    let mut name = format!("{base}.bak");
    let mut n = 1;
    while path.with_file_name(&name).exists() {
        name = format!("{base}-{n}.bak");
        n += 1;
    }
    let dest = path.with_file_name(&name);
    std::fs::copy(path, &dest).ok()?;
    Some(name)
}

/// The full release-truth repair plan: what to say (changes), which details to
/// recover (normalized), and the index to write (generated). Computed
/// identically by the status side and the apply side so advertised and applied
/// repairs cannot diverge.
pub struct ReleaseTruthPlan {
    pub changes: Vec<String>,
    /// (release-dir-relative file, recovered detail, backfilled field names)
    pub normalized: Vec<(String, Value, Vec<String>)>,
    pub entries: Value,
    pub generated: Value,
    pub index_changed: bool,
}

pub fn release_truth_repair_plan(
    release_dir: &Path,
    index_path: &Path,
    index: &Value,
) -> ReleaseTruthPlan {
    let now = chrono_now();
    // Components-normalized join: drops "." segments so "./v1.json" and
    // "v1.json" compare equal, and subdirectory spellings stay distinct.
    let normalize_path = |file: &str| -> std::path::PathBuf {
        release_dir.join(file).components().collect()
    };

    let mut entries: Vec<Value> = Vec::new();
    let mut normalized: Vec<(String, Value, Vec<String>)> = Vec::new();
    let mut changes: Vec<String> = Vec::new();
    let mut named_paths: std::collections::HashSet<std::path::PathBuf> =
        std::collections::HashSet::new();
    let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Index-named files first: kept even when imperfect, but imperfect ones are
    // recovered (backfilled + re-marshalled) rather than summarized with nulls.
    // Dangling references (file deleted) drop out: deleting a detail file is an
    // intentional removal.
    for summary in index["releases"].as_array().into_iter().flatten() {
        let Some(file) = summary["file"].as_str() else { continue };
        named_paths.insert(normalize_path(file));
        let path = release_dir.join(file);
        if !path.exists() {
            continue;
        }
        let raw = read_json(&path);
        let detail = match raw {
            Some(d) if is_complete_detail(&d) => d,
            other => {
                let (norm, backfilled) = normalize_release_detail(other.as_ref(), summary, &now);
                changes.push(format!("normalize incomplete release detail {file}"));
                normalized.push((file.to_string(), norm.clone(), backfilled));
                norm
            }
        };
        if let Some(id) = detail["id"].as_str() {
            seen_ids.insert(id.to_string());
        }
        entries.push(json!({ "file": file, "detail": detail }));
    }

    // Dir-discovered details the index omits (strict shape filter), deduped by
    // normalized path and by id — validation has no duplicate-id check, so an
    // id entering twice would be written as silent corruption.
    for entry in release_detail_entries_from_dir(release_dir, index_path)
        .as_array()
        .into_iter()
        .flatten()
    {
        let file = entry["file"].as_str().unwrap_or_default();
        if named_paths.contains(&normalize_path(file)) {
            continue;
        }
        let id = entry["detail"]["id"].as_str().unwrap_or_default();
        if !seen_ids.insert(id.to_string()) {
            continue;
        }
        entries.push(entry.clone());
    }

    let entries = Value::Array(entries);
    let generated = release::generated_release_index(index, &entries);
    let gen_changes: Vec<String> = release::release_index_generation_changes(index, &generated)
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    let index_changed = !gen_changes.is_empty();
    changes.extend(gen_changes);

    ReleaseTruthPlan { changes, normalized, entries, generated, index_changed }
}
