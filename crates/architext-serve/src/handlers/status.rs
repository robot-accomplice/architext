//! Handler for `GET /api/status`.
//!
//! Returns `{ ok, status }` where `status` is the full output of
//! `collect_status(target, version, run_validation=true)` and
//! `ok = installed && !needsMigration && validation.ok !== false`.
//!
//! Port of `statusApiRequest` in `src/adapters/cli/architext-cli.mjs` (~line 1220).
//!
//! Cache-Control: none (matches JS — `sendJson` sets no Cache-Control header for status).

use axum::{
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Extension,
};
use serde_json::{json, Value};

use crate::AppState;

/// GET /api/status → `{"ok": bool, "status": <collectStatus output>}`
pub async fn get_status(Extension(state): Extension<AppState>) -> Response {
    let target = state.target_dir.as_path();
    let version = state.cli_version.as_str();

    let status = architext_core::status::collect_status(target, version, true);
    let ok = status_ok(&status);
    let payload = json!({ "ok": ok, "status": status });

    let mut headers = HeaderMap::new();
    headers.insert(
        "content-type",
        HeaderValue::from_static("application/json; charset=utf-8"),
    );

    let body = serde_json::to_string_pretty(&payload)
        .map(|s| format!("{s}\n"))
        .unwrap_or_else(|_| "{\"ok\":false}\n".to_string());

    (StatusCode::OK, headers, body).into_response()
}

/// `ok = installed && !needsMigration && validation?.ok !== false`
///
/// Port of the JS expression in `statusApiRequest`.
pub fn status_ok(status: &Value) -> bool {
    let installed = status["installed"].as_bool().unwrap_or(false);
    let needs_migration = status["needsMigration"].as_bool().unwrap_or(false);
    // validation?.ok !== false — only false when explicitly `false` (null/missing → passes)
    let validation_failed = status["validation"]["ok"].as_bool() == Some(false);
    installed && !needs_migration && !validation_failed
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn status_ok_installed_no_migration_no_validation() {
        let s = json!({
            "installed": true,
            "needsMigration": false,
            "validation": null
        });
        assert!(status_ok(&s));
    }

    #[test]
    fn status_ok_installed_with_validation_ok_true() {
        let s = json!({
            "installed": true,
            "needsMigration": false,
            "validation": { "ok": true }
        });
        assert!(status_ok(&s));
    }

    #[test]
    fn status_not_ok_validation_failed() {
        let s = json!({
            "installed": true,
            "needsMigration": false,
            "validation": { "ok": false }
        });
        assert!(!status_ok(&s));
    }

    #[test]
    fn status_not_ok_not_installed() {
        let s = json!({
            "installed": false,
            "needsMigration": false,
            "validation": null
        });
        assert!(!status_ok(&s));
    }

    #[test]
    fn status_not_ok_needs_migration() {
        let s = json!({
            "installed": true,
            "needsMigration": true,
            "validation": { "ok": true }
        });
        assert!(!status_ok(&s));
    }

    #[test]
    fn status_ok_validation_null_ok_field() {
        // validation.ok = null → !== false → passes
        let s = json!({
            "installed": true,
            "needsMigration": false,
            "validation": { "ok": null }
        });
        assert!(status_ok(&s));
    }
}
