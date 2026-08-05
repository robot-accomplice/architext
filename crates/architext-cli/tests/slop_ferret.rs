use std::path::PathBuf;
use std::process::Command;

fn bin() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("../../target/debug/architext");
    path
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("slop-ferret")
}

fn replace_sha(text: &str, sha: &str) -> String {
    text.replace("PLACEHOLDER_SHA", sha)
}

fn write_mock_ferret(bin_dir: &PathBuf, sha: &str) {
    let script = format!(
        r#"#!/bin/sh
if [ "$1" = "--version" ] || [ "$1" = "version" ]; then
  echo "ferret 0.1.0"
  exit 0
fi
if [ "$1" != "enumerate" ]; then
  echo "ferret: unknown command $1" >&2
  exit 2
fi
shift
# Accept --no-record if present
if [ "$1" = "--no-record" ]; then
  shift
fi
if [ $# -lt 2 ]; then
  echo "ferret: enumerate requires plan and discharge" >&2
  exit 2
fi
PLAN="$1"
DISCHARGE="$2"
cat <<JSON
{{
  "plan_sha": "{sha}",
  "attested": {{
    "repo": "2/2",
    "repo_pct": 100.0,
    "repo_note": "files the auditor STATES they read",
    "plan": "1/1",
    "plan_pct": 100.0,
    "plan_note": "items the plan raised for which the discharge states a disposition",
    "waived": 0,
    "unclassified": 0
  }},
  "h_worklist_total": 1,
  "h_required_total": 1,
  "h_paths_attested": 1,
  "h_required_unattested": [],
  "h_deferred_unattested": 0,
  "change_baseline": "",
  "unmatched_changes_total": 0,
  "unmatched_changes_open": [],
  "unread_unmatched": [],
  "unread_unmatched_total": 0,
  "candidates_total": 0,
  "candidates_cleared": 0,
  "candidates_refuted": 0,
  "candidates_filed": 0,
  "filed_without_bar": [],
  "candidates_unaccounted": [],
  "unseeded_families": [],
  "families_declared_not_run": [],
  "remaining": [],
  "accounting": "complete",
  "headline": "auditor states 2/2 source files read · 1/1 of the plan dispositioned · nothing left open"
}}
JSON
"#,
        sha = sha
    );
    std::fs::create_dir_all(bin_dir).unwrap();
    let ferret_path = bin_dir.join("ferret");
    std::fs::write(&ferret_path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&ferret_path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&ferret_path, perms).unwrap();
    }
}

fn git_commit_sha(repo: &PathBuf) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("git rev-parse succeeds");
    assert!(out.status.success(), "git rev-parse failed");
    String::from_utf8(out.stdout)
        .unwrap()
        .trim()
        .to_string()
}

#[test]
fn bundles_slop_ferret_snapshot() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().join("docs").join("architext").join("data");
    std::fs::create_dir_all(&data_dir).unwrap();

    // Initialize a git repo so provenance (sha date) resolves.
    Command::new("git")
        .arg("-C")
        .arg(tmp.path())
        .args(["init", "--quiet"])
        .output()
        .expect("git init succeeds");
    Command::new("git")
        .arg("-C")
        .arg(tmp.path())
        .args(["config", "user.email", "test@example.com"])
        .output()
        .unwrap();
    Command::new("git")
        .arg("-C")
        .arg(tmp.path())
        .args(["config", "user.name", "Test"])
        .output()
        .unwrap();
    std::fs::write(tmp.path().join("file.txt"), "hello").unwrap();
    Command::new("git")
        .arg("-C")
        .arg(tmp.path())
        .args(["add", "file.txt"])
        .output()
        .unwrap();
    Command::new("git")
        .arg("-C")
        .arg(tmp.path())
        .args(["commit", "--quiet", "-m", "initial"])
        .output()
        .expect("git commit succeeds");
    let sha = git_commit_sha(&tmp.path().to_path_buf());

    let fixture = fixture_dir();
    let plan = replace_sha(&std::fs::read_to_string(fixture.join("plan.json")).unwrap(), &sha);
    let discharge =
        replace_sha(&std::fs::read_to_string(fixture.join("discharge.json")).unwrap(), &sha);
    let plan_path = tmp.path().join("plan.json");
    let discharge_path = tmp.path().join("discharge.json");
    let findings_path = tmp.path().join("findings.json");
    std::fs::write(&plan_path, plan).unwrap();
    std::fs::write(&discharge_path, discharge).unwrap();
    std::fs::copy(fixture.join("findings.json"), &findings_path).unwrap();

    std::fs::copy(fixture.join("manifest.json"), data_dir.join("manifest.json")).unwrap();
    for name in [
        "nodes.json",
        "flows.json",
        "views.json",
        "data-classification.json",
        "decisions.json",
        "risks.json",
        "glossary.json",
    ] {
        std::fs::write(
            data_dir.join(name),
            b"{\"nodes\":[],\"flows\":[],\"views\":[],\"classes\":[],\"decisions\":[],\"risks\":[],\"terms\":[]}",
        )
        .ok();
    }

    let mock_bin = tmp.path().join("bin");
    write_mock_ferret(&mock_bin, &sha);

    let original_path = std::env::var("PATH").unwrap_or_default();
    let mut paths: Vec<PathBuf> = std::env::split_paths(&original_path).collect();
    paths.insert(0, mock_bin);
    let new_path = std::env::join_paths(paths).unwrap();

    let output = Command::new(bin())
        .arg("slop-ferret")
        .arg(tmp.path())
        .arg("--plan")
        .arg(&plan_path)
        .arg("--discharge")
        .arg(&discharge_path)
        .arg("--findings")
        .arg(&findings_path)
        .env("PATH", new_path)
        .output()
        .expect("architext slop-ferret runs");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "unexpected exit: stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        stderr
    );

    let snapshot_path = data_dir.join("slop-ferret.json");
    assert!(snapshot_path.exists(), "snapshot not written");

    let text = std::fs::read_to_string(&snapshot_path).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(doc["schema"], 1);
    assert_eq!(doc["sha"], sha);
    assert_eq!(doc["date"].as_str().unwrap().len(), 10);
    assert_eq!(doc["attested_repo"], "2/2");
    assert_eq!(doc["attested_plan"], "1/1");
    assert_eq!(doc["accounting"], "complete");
    assert_eq!(doc["findings"].as_array().unwrap().len(), 1);

    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(data_dir.join("manifest.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["files"]["slopFerret"], "slop-ferret.json");
}
