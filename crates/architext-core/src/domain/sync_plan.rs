//! Pure port of `src/adapters/cli/sync-plan.mjs`.
//!
//! All functions are pure boolean/object logic; I/O-free.
//! Inputs and outputs are `serde_json::Value` to match the fixture contract.

use serde_json::{json, Map, Value};

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// `Boolean(v)` — JS truthiness for a JSON Value.
fn js_bool(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0 && !f.is_nan()).unwrap_or(false),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// Extract a string array from a JSON Value; returns empty vec on null/missing.
fn str_vec(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default()
}

// ─── Public API ───────────────────────────────────────────────────────────────

/// `normalizeSyncInstructionFiles(files, validInstructionFiles)`.
pub fn normalize_sync_instruction_files(files: &Value, valid_instruction_files: &[String]) -> Value {
    // files?.includes(fileName) — null/absent files → empty
    let files_arr = match files.as_array() {
        Some(a) => a,
        None => return Value::Array(vec![]),
    };
    let result: Vec<Value> = valid_instruction_files
        .iter()
        .filter(|name| files_arr.iter().any(|f| f.as_str() == Some(name.as_str())))
        .map(|name| Value::String(name.clone()))
        .collect();
    Value::Array(result)
}

/// `defaultSyncChoices({ instructionFiles })`.
pub fn default_sync_choices(instruction_files: &[String]) -> Value {
    json!({
        "branch": "current",
        "instructionFiles": instruction_files,
        "manageGitignore": true,
        "applyDoctorRepairs": true,
        "proceedWithChanges": true,
        "promptBeforeProceed": false
    })
}

/// `rememberedSyncChoices(metadata, { instructionFiles })`.
/// Returns `null` (Value::Null) if metadata or its syncChoices is absent/invalid.
pub fn remembered_sync_choices(metadata: &Value, instruction_files: &[String]) -> Value {
    let choices = match metadata.get("syncChoices") {
        Some(c) if c.is_object() => c,
        _ => return Value::Null,
    };

    // branch: must be one of "current"/"new"/"none"
    let branch = choices["branch"].as_str().unwrap_or("");
    let branch = if ["current", "new", "none"].contains(&branch) {
        branch.to_string()
    } else {
        "current".to_string()
    };

    // instructionFiles: filter against valid list
    let normalized = normalize_sync_instruction_files(&choices["instructionFiles"], instruction_files);

    // Boolean(choices.manageGitignore)
    let manage_gitignore = js_bool(&choices["manageGitignore"]);
    // applyDoctorRepairs !== false
    let apply_doctor_repairs = choices["applyDoctorRepairs"] != Value::Bool(false);
    let proceed_with_changes = choices["proceedWithChanges"] != Value::Bool(false);

    json!({
        "branch": branch,
        "instructionFiles": normalized,
        "manageGitignore": manage_gitignore,
        "applyDoctorRepairs": apply_doctor_repairs,
        "proceedWithChanges": proceed_with_changes,
        "promptBeforeProceed": false
    })
}

/// `applyExplicitSyncOptions(choices, options, { instructionFiles })`.
pub fn apply_explicit_sync_options(choices: &Value, options: &Value, instruction_files: &[String]) -> Value {
    let mut next = choices.as_object().cloned().unwrap_or_default();

    if let Some(branch) = options["branch"].as_str() {
        if !branch.is_empty() {
            next.insert("branch".to_string(), Value::String(branch.to_string()));
        }
    }

    // noAgents beats appendAgents
    if js_bool(&options["noAgents"]) {
        next.insert("instructionFiles".to_string(), Value::Array(vec![]));
    } else if js_bool(&options["appendAgents"]) {
        let files: Vec<Value> = instruction_files.iter().map(|f| Value::String(f.clone())).collect();
        next.insert("instructionFiles".to_string(), Value::Array(files));
    }

    // noGitignore beats updateGitignore
    if js_bool(&options["noGitignore"]) {
        next.insert("manageGitignore".to_string(), Value::Bool(false));
    } else if js_bool(&options["updateGitignore"]) {
        next.insert("manageGitignore".to_string(), Value::Bool(true));
    }

    Value::Object(next)
}

/// `syncOperation({ installing, migrating })`.
pub fn sync_operation(installing: bool, migrating: bool) -> &'static str {
    if installing { "install" } else if migrating { "migrate" } else { "sync" }
}

/// `syncWritePlan({ installing, migrating, doctorRepairAvailable, syncChoices, options })`.
pub fn sync_write_plan(
    installing: bool,
    migrating: bool,
    doctor_repair_available: bool,
    sync_choices: &Value,
    options: &Value,
) -> Value {
    let doctor_repairs_selected = doctor_repair_available
        && sync_choices["applyDoctorRepairs"] != Value::Bool(false);

    let instruction_files_len = sync_choices["instructionFiles"]
        .as_array()
        .map(|a| a.len())
        .unwrap_or(0);

    let should_write = installing
        || migrating
        || doctor_repairs_selected
        || js_bool(&options["force"])
        || instruction_files_len > 0
        || sync_choices["manageGitignore"] == Value::Bool(true);

    let operation = sync_operation(installing, migrating);
    let operation_label = if should_write {
        format!("Operation: {}", operation)
    } else {
        format!("Operation: {} (current)", operation)
    };

    json!({
        "doctorRepairsSelected": doctor_repairs_selected,
        "shouldWrite": should_write,
        "operation": operation,
        "operationLabel": operation_label
    })
}

/// `shouldValidateSync({ options, installing })`.
pub fn should_validate_sync(options: &Value, installing: bool) -> bool {
    !(js_bool(&options["skipValidate"]) || (js_bool(&options["dryRun"]) && installing))
}

/// `persistedSyncChoices(choices)`.
pub fn persisted_sync_choices(choices: &Value) -> Value {
    json!({
        "branch": choices["branch"],
        "instructionFiles": choices["instructionFiles"],
        "manageGitignore": choices["manageGitignore"],
        "applyDoctorRepairs": choices["applyDoctorRepairs"],
        "proceedWithChanges": choices["proceedWithChanges"]
    })
}

/// `syncMetadataPatch({ version, installing, migrating, instructionFiles, syncChoices,
///   managedInstructions, gitignoreManaged, validation, now })`.
///
/// `lastValidation: validation ? { ok, at } : undefined` — undefined drops key.
#[allow(clippy::too_many_arguments)]
pub fn sync_metadata_patch(
    version: &str,
    installing: bool,
    migrating: bool,
    instruction_files: &[String],
    sync_choices: &Value,
    managed_instructions: &Value,
    gitignore_managed: bool,
    validation: &Value,  // null or object
    now: &str,
) -> Value {
    let operation = sync_operation(installing, migrating);
    let data_policy = if installing { "starter-written" } else { "preserved" };

    // instructionFiles: Object.fromEntries(instructionFiles.map(...))
    let sync_choices_instruction_files: Vec<String> = str_vec(&sync_choices["instructionFiles"]);
    let mut instr_obj = Map::new();
    for fname in instruction_files {
        let included = sync_choices_instruction_files.contains(fname);
        instr_obj.insert(fname.clone(), Value::Bool(included));
    }

    let mut patch = json!({
        "source": "architext-cli",
        "cliVersion": version,
        "operation": operation,
        "dataPolicy": data_policy,
        "copiedInstallMigrated": migrating,
        "instructionFiles": Value::Object(instr_obj),
        "managedInstructions": managed_instructions,
        "gitignoreManaged": gitignore_managed,
        "syncChoices": persisted_sync_choices(sync_choices)
    });

    // lastValidation: validation ? { ok, at: now } : undefined → key dropped
    if !validation.is_null() {
        let ok = js_bool(&validation["ok"]);
        patch.as_object_mut().unwrap().insert(
            "lastValidation".to_string(),
            json!({ "ok": ok, "at": now }),
        );
    }

    patch
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalize_filters_to_valid() {
        let files = json!(["CLAUDE.md", "AGENTS.md", "extra.md"]);
        let valid = vec!["AGENTS.md".to_string(), "CLAUDE.md".to_string()];
        let result = normalize_sync_instruction_files(&files, &valid);
        assert_eq!(result, json!(["AGENTS.md", "CLAUDE.md"]));
    }

    #[test]
    fn normalize_null_files() {
        let valid = vec!["AGENTS.md".to_string()];
        let result = normalize_sync_instruction_files(&Value::Null, &valid);
        assert_eq!(result, json!([]));
    }

    #[test]
    fn default_choices() {
        let result = default_sync_choices(&["CLAUDE.md".to_string()]);
        assert_eq!(result["manageGitignore"], true);
        assert_eq!(result["branch"], "current");
    }

    #[test]
    fn remembered_choices_null_metadata() {
        let result = remembered_sync_choices(&Value::Null, &[]);
        assert_eq!(result, Value::Null);
    }

    #[test]
    fn remembered_choices_invalid_branch_fallback() {
        let meta = json!({ "syncChoices": { "branch": "invalid", "instructionFiles": [], "manageGitignore": true, "applyDoctorRepairs": true, "proceedWithChanges": true } });
        let result = remembered_sync_choices(&meta, &["CLAUDE.md".to_string()]);
        assert_eq!(result["branch"], "current");
    }

    #[test]
    fn sync_operation_values() {
        assert_eq!(sync_operation(true, false), "install");
        assert_eq!(sync_operation(false, true), "migrate");
        assert_eq!(sync_operation(false, false), "sync");
    }

    #[test]
    fn write_plan_install_should_write() {
        let choices = json!({ "instructionFiles": [], "manageGitignore": false, "applyDoctorRepairs": false });
        let result = sync_write_plan(true, false, false, &choices, &json!({}));
        assert_eq!(result["shouldWrite"], true);
        assert_eq!(result["operation"], "install");
    }

    #[test]
    fn write_plan_sync_current() {
        let choices = json!({ "instructionFiles": [], "manageGitignore": false, "applyDoctorRepairs": false });
        let result = sync_write_plan(false, false, false, &choices, &json!({}));
        assert_eq!(result["shouldWrite"], false);
        assert_eq!(result["operationLabel"], "Operation: sync (current)");
    }

    #[test]
    fn should_validate_normal() {
        assert!(should_validate_sync(&json!({}), false));
    }

    #[test]
    fn should_validate_skip() {
        assert!(!should_validate_sync(&json!({ "skipValidate": true }), false));
    }

    #[test]
    fn metadata_patch_drops_last_validation_when_null() {
        let sync_choices = json!({ "branch": "current", "instructionFiles": ["CLAUDE.md"], "manageGitignore": true, "applyDoctorRepairs": true, "proceedWithChanges": true });
        let result = sync_metadata_patch("1.7.0", true, false, &["CLAUDE.md".to_string()], &sync_choices, &json!([]), false, &Value::Null, "2024-01-01T00:00:00.000Z");
        assert!(result.get("lastValidation").is_none());
    }

    #[test]
    fn metadata_patch_includes_last_validation() {
        let sync_choices = json!({ "branch": "current", "instructionFiles": ["CLAUDE.md"], "manageGitignore": true, "applyDoctorRepairs": true, "proceedWithChanges": true });
        let result = sync_metadata_patch("1.7.0", false, false, &["CLAUDE.md".to_string()], &sync_choices, &json!(["CLAUDE.md"]), true, &json!({ "ok": true }), "2024-01-01T00:00:00.000Z");
        assert_eq!(result["lastValidation"]["ok"], true);
    }
}
