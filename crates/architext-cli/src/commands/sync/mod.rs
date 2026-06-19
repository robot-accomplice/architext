//! `sync` / `install` / `upgrade` / `migrate` command — Rust port of
//! `syncTarget` from `src/adapters/cli/architext-cli.mjs`.
//!
//! Sub-modules:
//!   - `target_layout`   — path helpers + constants
//!   - `timestamp`       — `now_iso()`
//!   - `starter_data`    — `write_starter_data` + `write_starter_release_data`
//!   - `instruction_files` — `upsert_instruction_file`
//!   - `gitignore`       — `upsert_gitignore`
//!   - `root_scripts`    — `upsert_root_scripts`
//!   - `metadata`        — `read_metadata` / `write_metadata`
//!   - `branch`          — `handle_branch`
//!   - `status_printer`  — verbose `format_verbose_status_lines`

mod branch;
mod gitignore;
mod instruction_files;
mod metadata;
mod root_scripts;
mod starter_data;
pub mod status_printer;
mod target_layout;
mod timestamp;

use std::path::Path;
use std::process;

use architext_core::domain::doctor_repairs::apply_doctor_repairs;
use architext_core::domain::sync_plan::{
    apply_explicit_sync_options, default_sync_choices, should_validate_sync,
    sync_metadata_patch, sync_write_plan,
};
use architext_core::status::collect_status;
use serde_json::{json, Value};

use crate::args::ParsedArgs;
use crate::commands::status::format_status_lines;

use self::branch::handle_branch;
use self::gitignore::upsert_gitignore;
use self::instruction_files::upsert_instruction_file;
use self::metadata::{read_metadata, write_metadata};
use self::root_scripts::upsert_root_scripts;
use self::starter_data::write_starter_data;
use self::status_printer::format_verbose_status_lines;
use self::target_layout::{
    copied_install_candidate_paths, data_dir, INSTRUCTION_FILES,
};

fn println_single(line: &str) {
    println!("{line}");
}

/// Port of `copiedInstallPaths(target)` — existing copied-install entries.
fn copied_install_paths(target: &Path) -> Vec<String> {
    copied_install_candidate_paths(target)
        .into_iter()
        .filter(|p| p.exists())
        .filter_map(|p| {
            p.strip_prefix(target)
                .ok()
                .map(|r| r.to_string_lossy().to_string())
        })
        .collect()
}

/// Port of `removeCopiedInstallFiles(target, dryRun)`.
fn remove_copied_install_files(target: &Path, dry_run: bool) -> Vec<String> {
    use self::target_layout::legacy_metadata_path;
    let mut removed = Vec::new();
    for entry_path in copied_install_candidate_paths(target) {
        if !entry_path.exists() {
            continue;
        }
        let rel = entry_path
            .strip_prefix(target)
            .ok()
            .map(|r| r.to_string_lossy().to_string())
            .unwrap_or_default();
        removed.push(rel);
        if !dry_run {
            let _ = std::fs::remove_dir_all(&entry_path)
                .or_else(|_| std::fs::remove_file(&entry_path));
        }
    }
    let legacy = legacy_metadata_path(target);
    if legacy.exists() {
        let rel = legacy
            .strip_prefix(target)
            .ok()
            .map(|r| r.to_string_lossy().to_string())
            .unwrap_or_default();
        removed.push(rel);
        if !dry_run {
            let _ = std::fs::remove_file(&legacy);
        }
    }
    removed
}

/// Normalise the `options` Value from `ParsedArgs` into the shape used by the
/// `sync_plan` domain functions.
fn options_json(opts: &ParsedArgs) -> Value {
    json!({
        "branch": opts.branch,
        "noAgents": opts.no_agents,
        "appendAgents": opts.append_agents,
        "noGitignore": opts.no_gitignore,
        "updateGitignore": opts.update_gitignore,
        "noRootScripts": opts.no_root_scripts,
        "rootScripts": opts.root_scripts,
        "dryRun": opts.dry_run,
        "force": opts.force,
        "overwriteData": opts.overwrite_data,
        "skipValidate": opts.skip_validate,
        "yes": opts.yes,
        "quiet": opts.quiet,
        "prompt": opts.prompt
    })
}

/// Run the `sync` command.
pub fn run(target: &Path, opts: &ParsedArgs, version: &str) {
    // assertSyncPromptOptions
    if opts.prompt && (opts.yes || opts.quiet) {
        eprintln!("--prompt cannot be combined with --yes or --quiet");
        process::exit(1);
    }

    let options = options_json(opts);
    let run_validation = !opts.skip_validate;

    // collectStatus
    let status = collect_status(target, version, run_validation);

    let installing = !status["installed"].as_bool().unwrap_or(false) || opts.overwrite_data;
    let migrating = status["needsMigration"].as_bool().unwrap_or(false);

    // effectiveDoctorRepairs: filter out instruction-rules if --no-agents
    let all_repairs = status["doctorRepairs"].as_array().cloned().unwrap_or_default();
    let effective_repairs: Vec<Value> = if opts.no_agents {
        all_repairs
            .into_iter()
            .filter(|r| r["category"].as_str() != Some("instruction-rules"))
            .collect()
    } else {
        all_repairs
    };
    let doctor_repair_available = !effective_repairs.is_empty();

    println_single(&format!("Target: {}", target.display()));
    println_single(&format!("Architext CLI: {version}"));

    if opts.dry_run {
        // JS: if (options.dryRun) printStatus(status, { verbose: true }, logger)
        for line in format_verbose_status_lines(&status) {
            println_single(&line);
        }
    }

    if migrating {
        let count = copied_install_paths(target).len();
        println_single(&format!("Copied install detected: {count} package-owned paths"));
    }

    // Select sync choices (non-interactive: --yes path only in gate)
    let instruction_files_list: Vec<String> = INSTRUCTION_FILES.iter().map(|s| s.to_string()).collect();
    let metadata_val = read_metadata(target).unwrap_or(Value::Null);
    let root_package_exists = target.join("package.json").exists();

    let defaults = default_sync_choices(root_package_exists, &instruction_files_list);
    // Non-interactive: use defaults + explicit options
    let sync_choices = apply_explicit_sync_options(&defaults, &options, &instruction_files_list);

    // syncWritePlan
    let write_plan = sync_write_plan(
        installing,
        migrating,
        doctor_repair_available,
        &sync_choices,
        &options,
    );
    let should_write = write_plan["shouldWrite"].as_bool().unwrap_or(false);
    let doctor_repairs_selected = write_plan["doctorRepairsSelected"].as_bool().unwrap_or(false);

    println_single(write_plan["operationLabel"].as_str().unwrap_or(""));

    if !should_write {
        if !opts.dry_run {
            for line in format_status_lines(&status) {
                println_single(&line);
            }
        }
        println_single("No lifecycle changes needed.");
        return;
    }

    // Perform writes (dry-run or real)
    perform_writes(
        target,
        opts,
        version,
        &status,
        &sync_choices,
        &options,
        &instruction_files_list,
        installing,
        migrating,
        doctor_repairs_selected,
        &metadata_val,
    );
}

#[allow(clippy::too_many_arguments)]
fn perform_writes(
    target: &Path,
    opts: &ParsedArgs,
    version: &str,
    status: &Value,
    sync_choices: &Value,
    options: &Value,
    instruction_files_list: &[String],
    installing: bool,
    migrating: bool,
    doctor_repairs_selected: bool,
    _metadata_val: &Value,
) {
    let dry_run = opts.dry_run;

    // handleBranch
    let branch_choice = sync_choices["branch"].as_str().unwrap_or("current");
    let branch_name_override = if opts.branch_name.is_empty() { None } else { Some(opts.branch_name.as_str()) };
    match handle_branch(target, branch_choice, dry_run, version, branch_name_override) {
        Ok(Some(branch_name)) => {
            println_single(&format!("Created and switched to branch {branch_name}"));
        }
        Ok(None) => {}
        Err(e) => {
            eprintln!("{e}");
            process::exit(1);
        }
    }

    // Starter data
    if installing {
        let data_path = data_dir(target);
        println_single(&format!(
            "{} starter data to {}",
            if dry_run { "Would write" } else { "Writing" },
            data_path.display()
        ));
        if !dry_run {
            if let Err(e) = write_starter_data(target) {
                eprintln!("Failed to write starter data: {e}");
                process::exit(1);
            }
        }
    } else {
        println_single("Preserving target-owned docs/architext/data/**/*.json");
    }

    // removeCopiedInstallFiles
    if migrating {
        let removed = remove_copied_install_files(target, dry_run);
        if !removed.is_empty() {
            println_single(&format!(
                "{} copied package-owned files:",
                if dry_run { "Would remove" } else { "Removed" }
            ));
            for item in &removed {
                println_single(&format!("- {item}"));
            }
        }
    }

    // applyDoctorRepairs
    if !installing && doctor_repairs_selected {
        let skip_ir = opts.no_agents;
        if dry_run {
            // collect from status.doctorRepairs
            let repairs = status["doctorRepairs"].as_array().cloned().unwrap_or_default();
            println_single("Would apply doctor repairs:");
            for repair in &repairs {
                let file = repair["file"].as_str().unwrap_or("");
                let summary = repair["summary"].as_str().unwrap_or("");
                println_single(&format!("- {file}: {summary}"));
            }
        } else {
            let repairs = apply_doctor_repairs(target, status, dry_run, skip_ir);
            println_single("Applied doctor repairs:");
            for repair in &repairs {
                println_single(&format!("- {}: {}", repair.file, repair.summary));
            }
        }
    } else if !installing && !doctor_repairs_selected && status["doctorRepairs"].as_array().map(|a| !a.is_empty()).unwrap_or(false) {
        println_single("Skipped doctor repairs.");
    }

    // upsertInstructionFile for each selected fileName
    let selected_instruction_files: Vec<String> = sync_choices["instructionFiles"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();

    let mut managed_instructions: Vec<String> = Vec::new();
    for file_name in &selected_instruction_files {
        match upsert_instruction_file(target, file_name, dry_run) {
            Ok((changed, _created)) => {
                if changed {
                    let dest = target.join(file_name);
                    println_single(&format!(
                        "{} {}",
                        if dry_run { "Would update" } else { "Updated" },
                        dest.display()
                    ));
                    managed_instructions.push(file_name.clone());
                } else {
                    let dest = target.join(file_name);
                    println_single(&format!("Skipped {}: already current", dest.display()));
                }
            }
            Err(e) => {
                eprintln!("Failed to update {file_name}: {e}");
                process::exit(1);
            }
        }
    }

    // upsertGitignore
    let mut gitignore_managed = false;
    if sync_choices["manageGitignore"].as_bool().unwrap_or(false) {
        match upsert_gitignore(target, dry_run) {
            Ok((changed, _missing)) => {
                let dest = target.join(".gitignore");
                if changed {
                    println_single(&format!(
                        "{} {}",
                        if dry_run { "Would update" } else { "Updated" },
                        dest.display()
                    ));
                    gitignore_managed = true;
                } else {
                    println_single(&format!("Skipped {}: already present", dest.display()));
                    // JS: gitignoreManaged = result.changed || result.reason === "already present"
                    gitignore_managed = true;
                }
            }
            Err(e) => {
                eprintln!("Failed to update .gitignore: {e}");
                process::exit(1);
            }
        }
    }

    // upsertRootScripts
    let mut root_scripts_managed = false;
    if sync_choices["manageRootScripts"].as_bool().unwrap_or(false) {
        match upsert_root_scripts(target, dry_run) {
            Ok((changed, missing)) => {
                let dest = target.join("package.json");
                if changed {
                    println_single(&format!(
                        "{} {} with {} scripts",
                        if dry_run { "Would update" } else { "Updated" },
                        dest.display(),
                        missing.len()
                    ));
                    root_scripts_managed = true;
                } else {
                    let reason = if !target.join("package.json").exists() {
                        "missing package.json"
                    } else {
                        "already present"
                    };
                    println_single(&format!("Skipped {}: {reason}", dest.display()));
                    root_scripts_managed = reason == "already present";
                }
            }
            Err(e) => {
                eprintln!("Failed to update package.json: {e}");
                process::exit(1);
            }
        }
    }

    // shouldValidateSync — skip validation in dry-run-install or skipValidate
    let do_validate = should_validate_sync(options, installing) && !dry_run;
    let validation_result: Value = if do_validate {
        // Run validation via architext_core
        let schema_dir = std::path::PathBuf::from("viewer/schema");
        let schema_dir = if schema_dir.exists() {
            schema_dir
        } else if let Ok(s) = std::env::var("ARCHITEXT_SCHEMA_DIR") {
            std::path::PathBuf::from(s)
        } else {
            schema_dir
        };
        let target_data_dir = data_dir(target);
        if target_data_dir.join("manifest.json").exists() && schema_dir.exists() {
            let outcome = architext_core::validate_data_dir(&target_data_dir, &schema_dir);
            println_single(&format!("Validation: {}", if outcome.ok { "passed" } else { "failed" }));
            serde_json::json!({ "ok": outcome.ok })
        } else {
            Value::Null
        }
    } else {
        Value::Null
    };

    if !dry_run {
        // writeMetadata
        let patch = sync_metadata_patch(
            version,
            installing,
            migrating,
            instruction_files_list,
            sync_choices,
            &Value::Array(managed_instructions.iter().map(|s| Value::String(s.clone())).collect()),
            gitignore_managed,
            root_scripts_managed,
            &validation_result,
            &timestamp::now_iso(),
        );
        if let Err(e) = write_metadata(target, &patch) {
            eprintln!("Failed to write metadata: {e}");
            process::exit(1);
        }

        // Final status print (verbose)
        let final_status = collect_status(target, version, !opts.skip_validate && do_validate);
        for line in format_verbose_status_lines(&final_status) {
            println_single(&line);
        }
    }
}
