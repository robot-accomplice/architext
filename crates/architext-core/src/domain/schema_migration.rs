//! Pure port of `src/domain/lifecycle/schema-migrations.mjs`.

use serde_json::{json, Value};

#[derive(Debug, PartialEq)]
struct Version {
    major: u64,
    minor: u64,
    patch: u64,
}

/// `parseVersion(version)` — returns None if not matching `^\d+\.\d+\.\d+$`.
/// Mirrors JS: `String(version ?? "").match(...)`.
fn parse_version(version: &str) -> Option<Version> {
    // Split on '.' and require exactly 3 numeric segments
    let parts: Vec<&str> = version.splitn(4, '.').collect();
    if parts.len() != 3 {
        return None;
    }
    let major = parts[0].parse::<u64>().ok()?;
    let minor = parts[1].parse::<u64>().ok()?;
    let patch = parts[2].parse::<u64>().ok()?;
    // Verify no trailing content — each part must be purely digits
    if !parts[0].chars().all(|c| c.is_ascii_digit())
        || !parts[1].chars().all(|c| c.is_ascii_digit())
        || !parts[2].chars().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    // Edge: empty parts are invalid
    if parts[0].is_empty() || parts[1].is_empty() || parts[2].is_empty() {
        return None;
    }
    Some(Version { major, minor, patch })
}

/// `compareVersions(left, right)` — returns negative/zero/positive like JS.
fn compare_versions(left: &str, right: &str) -> i64 {
    let pl = parse_version(left);
    let pr = parse_version(right);
    if pl.is_none() || pr.is_none() {
        return 0;
    }
    let pl = pl.unwrap();
    let pr = pr.unwrap();
    for (l, r) in [(pl.major, pr.major), (pl.minor, pr.minor), (pl.patch, pr.patch)] {
        if l != r {
            return l as i64 - r as i64;
        }
    }
    0
}

/// `migrationKind(fromVersion, toVersion)`.
fn migration_kind(from_version: &str, to_version: &str) -> &'static str {
    let from = parse_version(from_version);
    let to = parse_version(to_version);
    if from.is_none() || to.is_none() {
        return "unknown";
    }
    let from = from.unwrap();
    let to = to.unwrap();
    if to.major > from.major {
        "breaking"
    } else {
        "additive"
    }
}

/// `schemaMigrationPlan({ currentVersion, targetVersion })`.
pub fn schema_migration_plan(current_version: &str, target_version: &str) -> Value {
    // JS: const current = currentVersion || ""; const target = targetVersion || "";
    let current = current_version;
    let target = target_version;

    let current_parsed = if current.is_empty() { None } else { parse_version(current) };
    let target_parsed = if target.is_empty() { None } else { parse_version(target) };

    // if (current && !currentParsed) → invalid current
    if !current.is_empty() && current_parsed.is_none() {
        return json!({
            "currentVersion": current,
            "targetVersion": target,
            "pending": [{
                "id": "schema-version-invalid-current",
                "kind": "invalid",
                "file": "docs/architext/data/manifest.json",
                "fromVersion": current,
                "toVersion": target,
                "summary": format!("target schemaVersion must be semantic version x.y.z; got {current}")
            }],
            "upToDate": false
        });
    }

    // if (target && !targetParsed) → invalid target
    if !target.is_empty() && target_parsed.is_none() {
        return json!({
            "currentVersion": current,
            "targetVersion": target,
            "pending": [{
                "id": "schema-version-invalid-target",
                "kind": "invalid",
                "file": "docs/architext/data/manifest.json",
                "fromVersion": current,
                "toVersion": target,
                "summary": format!("CLI schema version {target} is invalid; expected semantic version x.y.z")
            }],
            "upToDate": false
        });
    }

    // if (!target || current === target) → up to date
    if target.is_empty() || current == target {
        return json!({
            "currentVersion": current,
            "targetVersion": target,
            "pending": [],
            "upToDate": true
        });
    }

    // direction = compareVersions(current, target)
    let direction = compare_versions(current, target);
    if direction > 0 {
        return json!({
            "currentVersion": current,
            "targetVersion": target,
            "pending": [{
                "id": "schema-version-ahead",
                "kind": "unsupported",
                "file": "docs/architext/data/manifest.json",
                "fromVersion": current,
                "toVersion": target,
                "summary": format!("target schemaVersion {current} is newer than CLI schema {target}; install a newer Architext CLI before migrating")
            }],
            "upToDate": false
        });
    }

    // Additive or breaking migration
    let from_label = if current.is_empty() { "missing" } else { current };
    let kind = migration_kind(current, target);
    json!({
        "currentVersion": current,
        "targetVersion": target,
        "pending": [{
            "id": format!("schema-version-{from_label}-to-{target}"),
            "kind": kind,
            "file": "docs/architext/data/manifest.json",
            "fromVersion": current,
            "toVersion": target,
            "summary": format!("apply {kind} schema migration {from_label} -> {target}: update manifest.schemaVersion")
        }],
        "upToDate": false
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_valid() {
        let v = parse_version("1.2.3").unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 2);
        assert_eq!(v.patch, 3);
    }

    #[test]
    fn parse_version_invalid() {
        assert!(parse_version("1.2").is_none());
        assert!(parse_version("abc").is_none());
        assert!(parse_version("1.2.3.4").is_none());
        assert!(parse_version("").is_none());
        assert!(parse_version("1.2.x").is_none());
    }

    #[test]
    fn up_to_date_same_version() {
        let r = schema_migration_plan("1.0.0", "1.0.0");
        assert_eq!(r["upToDate"], true);
        assert_eq!(r["pending"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn up_to_date_no_target() {
        let r = schema_migration_plan("1.0.0", "");
        assert_eq!(r["upToDate"], true);
    }

    #[test]
    fn additive_migration() {
        let r = schema_migration_plan("1.0.0", "1.1.0");
        assert_eq!(r["upToDate"], false);
        assert_eq!(r["pending"][0]["kind"], "additive");
        assert_eq!(r["pending"][0]["id"], "schema-version-1.0.0-to-1.1.0");
    }

    #[test]
    fn breaking_migration() {
        let r = schema_migration_plan("1.0.0", "2.0.0");
        assert_eq!(r["upToDate"], false);
        assert_eq!(r["pending"][0]["kind"], "breaking");
    }

    #[test]
    fn invalid_current_version() {
        let r = schema_migration_plan("not-a-version", "1.0.0");
        assert_eq!(r["pending"][0]["id"], "schema-version-invalid-current");
    }

    #[test]
    fn invalid_target_version() {
        let r = schema_migration_plan("1.0.0", "not-a-version");
        assert_eq!(r["pending"][0]["id"], "schema-version-invalid-target");
    }

    #[test]
    fn version_ahead() {
        let r = schema_migration_plan("2.0.0", "1.0.0");
        assert_eq!(r["pending"][0]["id"], "schema-version-ahead");
        assert_eq!(r["pending"][0]["kind"], "unsupported");
    }

    #[test]
    fn missing_current_version() {
        let r = schema_migration_plan("", "1.0.0");
        assert_eq!(r["upToDate"], false);
        assert_eq!(r["pending"][0]["id"], "schema-version-missing-to-1.0.0");
    }
}
