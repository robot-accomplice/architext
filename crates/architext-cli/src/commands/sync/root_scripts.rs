//! Port of `upsertRootScripts({ target, dryRun })` from `architext-cli.mjs`.

use std::path::Path;

use architext_core::json_write::write_json_string;
use serde_json::Value;

use super::target_layout::ROOT_SCRIPTS;

fn read_package_json(target: &Path) -> Option<Value> {
    let path = target.join("package.json");
    if !path.exists() {
        return None;
    }
    let text = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Port of `upsertRootScripts`.
/// Returns `(changed, missing_script_names)`.
pub fn upsert_root_scripts(target: &Path, dry_run: bool) -> std::io::Result<(bool, Vec<String>)> {
    let pkg_path = target.join("package.json");
    let pkg = match read_package_json(target) {
        None => return Ok((false, vec![])),
        Some(v) => v,
    };

    let existing_scripts = pkg.get("scripts").and_then(|s| s.as_object()).cloned().unwrap_or_default();
    let missing: Vec<(&str, &str)> = ROOT_SCRIPTS
        .iter()
        .copied()
        .filter(|(name, value)| existing_scripts.get(*name).and_then(|v| v.as_str()) != Some(*value))
        .collect();

    if missing.is_empty() {
        return Ok((false, vec![]));
    }

    if !dry_run {
        // JS: { ...packageJson, scripts: { ...existingScripts, ...Object.fromEntries(missing) } }
        let mut new_pkg = pkg.clone();
        let scripts_map = new_pkg
            .as_object_mut()
            .unwrap()
            .entry("scripts")
            .or_insert_with(|| Value::Object(Default::default()))
            .as_object_mut()
            .unwrap();
        for (name, value) in &missing {
            scripts_map.insert((*name).to_string(), Value::String((*value).to_string()));
        }
        std::fs::write(&pkg_path, write_json_string(&new_pkg).as_bytes())?;
    }

    Ok((true, missing.iter().map(|(name, _)| name.to_string()).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_pkg(dir: &Path, scripts: &[(&str, &str)]) {
        let scripts_obj: serde_json::Map<String, Value> = scripts
            .iter()
            .map(|(k, v)| (k.to_string(), Value::String(v.to_string())))
            .collect();
        let pkg = serde_json::json!({ "name": "test", "scripts": scripts_obj });
        std::fs::write(dir.join("package.json"), write_json_string(&pkg)).unwrap();
    }

    #[test]
    fn no_package_json_no_change() {
        let dir = TempDir::new().unwrap();
        let (changed, _) = upsert_root_scripts(dir.path(), false).unwrap();
        assert!(!changed);
    }

    #[test]
    fn adds_missing_scripts() {
        let dir = TempDir::new().unwrap();
        write_pkg(dir.path(), &[]);
        let (changed, missing) = upsert_root_scripts(dir.path(), false).unwrap();
        assert!(changed);
        assert_eq!(missing.len(), ROOT_SCRIPTS.len());
        let pkg: Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("package.json")).unwrap()
        ).unwrap();
        for (name, value) in ROOT_SCRIPTS {
            assert_eq!(pkg["scripts"][name].as_str(), Some(*value));
        }
    }

    #[test]
    fn idempotent_after_add() {
        let dir = TempDir::new().unwrap();
        write_pkg(dir.path(), &[]);
        upsert_root_scripts(dir.path(), false).unwrap();
        let (changed, _) = upsert_root_scripts(dir.path(), false).unwrap();
        assert!(!changed);
    }
}
