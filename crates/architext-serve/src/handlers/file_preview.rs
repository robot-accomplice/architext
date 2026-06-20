//! Handler for `GET /api/file?path=<relpath>`.
//!
//! Returns a single repository file's CONTENTS with server-side syntax
//! highlighting, for the Repo Tree file-preview pane. The WASM viewer carries
//! no highlighter; it renders the inline-styled HTML this handler produces.
//!
//! The `path` is resolved UNDER THE SAME ROOT the repo-tree handler lists from
//! (`AppState::target_dir`, the served project/git root — see
//! `handlers/repo_tree.rs`), via `crate::safe_join::safe_join` so traversal
//! attempts are rejected before any filesystem access.
//!
//! Response JSON: `{ path, size, language, truncated, binary, html }`.
//!   - `binary: true` (NUL byte or invalid UTF-8) → `html` is null, no highlight.
//!   - `truncated: true` → only the head (`MAX_FILE_BYTES`) was read + highlighted.
//!
//! Highlighting uses `syntect` (a native crate — fine on the serve side; the
//! viewer stays zero-CSS for tokens because the HTML carries inline styles).
//! Syntax is chosen by file extension with a plain-text fallback; the theme is
//! the bundled `base16-ocean.dark`.
//!
//! FUTURE REFINEMENT: emit class-based HTML (`syntect::html::ClassedHTMLGenerator`)
//! and ship a facelift-palette token stylesheet, so highlight colors track the
//! viewer theme instead of syntect's bundled theme. Inline-styled HTML is used
//! for now to keep the viewer free of token CSS.
//!
//! Cache-Control: no-store (file contents are volatile, like repo-tree).

use std::path::Path;

use axum::{
    extract::Query,
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Extension,
};
use serde::Deserialize;
use serde_json::{json, Value};
use syntect::highlighting::ThemeSet;
use syntect::html::highlighted_html_for_string;
use syntect::parsing::SyntaxSet;

use crate::safe_join::safe_join;
use crate::AppState;

// ─── Constants ────────────────────────────────────────────────────────────────

/// Maximum number of bytes read + highlighted from a file. Larger files return
/// only this head with `truncated: true`. 512 KiB keeps highlight latency and
/// response size bounded for the preview pane.
const MAX_FILE_BYTES: usize = 512 * 1024;

/// Bundled dark theme used for inline-styled highlight HTML.
const THEME_NAME: &str = "base16-ocean.dark";

// ─── Query ──────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct FileQuery {
    /// Repository-relative path of the file to preview.
    pub path: Option<String>,
}

// ─── Outcome (RCA trail) ──────────────────────────────────────────────────────

/// The resolved outcome of a file-preview request, recorded in the structured
/// `tracing` trail so a failed/odd request can be root-caused after the fact
/// (Rule 14) rather than inferred from the response alone.
#[derive(Debug, Clone, Copy)]
enum Outcome {
    Ok,
    Truncated,
    Binary,
    Denied,
    NotFound,
    BadRequest,
}

impl Outcome {
    fn as_str(self) -> &'static str {
        match self {
            Outcome::Ok => "ok",
            Outcome::Truncated => "truncated",
            Outcome::Binary => "binary",
            Outcome::Denied => "denied",
            Outcome::NotFound => "not-found",
            Outcome::BadRequest => "bad-request",
        }
    }
}

// ─── Pure builder (testable without HTTP) ─────────────────────────────────────

/// The fully-resolved preview result: the JSON payload to serialize, the HTTP
/// status to return it with, and the RCA outcome to record.
struct PreviewResult {
    status: StatusCode,
    payload: Value,
    outcome: Outcome,
}

/// Resolve + read + highlight `request_path` under `root`. Pure (no HTTP, no
/// logging) so the behaviour matrix is unit-testable directly.
///
/// `syntax_set` / `theme_set` are passed in so the (relatively expensive)
/// default-load happens once per request at the call site, not per helper.
fn build_preview(
    root: &Path,
    request_path: &str,
    syntax_set: &SyntaxSet,
    theme_set: &ThemeSet,
) -> PreviewResult {
    // 1. Traversal guard — reject anything that escapes `root`.
    let resolved = match safe_join(root, request_path) {
        Some(p) => p,
        None => {
            return PreviewResult {
                status: StatusCode::BAD_REQUEST,
                payload: json!({ "error": "path escapes repository root" }),
                outcome: Outcome::Denied,
            };
        }
    };

    // 2. Must exist and be a regular file.
    let meta = match std::fs::metadata(&resolved) {
        Ok(m) if m.is_file() => m,
        _ => {
            return PreviewResult {
                status: StatusCode::NOT_FOUND,
                payload: json!({ "error": "file not found" }),
                outcome: Outcome::NotFound,
            };
        }
    };
    let size = meta.len();

    // 3. Read with the size cap. Larger files → head only + truncated flag.
    let raw = match read_capped(&resolved) {
        Ok(r) => r,
        Err(_) => {
            return PreviewResult {
                status: StatusCode::NOT_FOUND,
                payload: json!({ "error": "file not readable" }),
                outcome: Outcome::NotFound,
            };
        }
    };
    let truncated = (size as usize) > raw.len();

    // 4. Binary detection: a NUL byte or invalid UTF-8 → no highlight.
    if raw.contains(&0) {
        return PreviewResult {
            status: StatusCode::OK,
            payload: json!({
                "path": request_path,
                "size": size,
                "language": "binary",
                "truncated": truncated,
                "binary": true,
                "html": Value::Null,
            }),
            outcome: Outcome::Binary,
        };
    }
    let text = match String::from_utf8(raw) {
        Ok(t) => t,
        Err(_) => {
            return PreviewResult {
                status: StatusCode::OK,
                payload: json!({
                    "path": request_path,
                    "size": size,
                    "language": "binary",
                    "truncated": truncated,
                    "binary": true,
                    "html": Value::Null,
                }),
                outcome: Outcome::Binary,
            };
        }
    };

    // 5. Pick syntax by extension (plain-text fallback) and highlight.
    let extension = resolved
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let syntax = syntax_set
        .find_syntax_by_extension(extension)
        .unwrap_or_else(|| syntax_set.find_syntax_plain_text());
    let language = syntax.name.clone();
    let theme = &theme_set.themes[THEME_NAME];

    let html = highlighted_html_for_string(&text, syntax_set, syntax, theme)
        .unwrap_or_else(|_| String::new());

    PreviewResult {
        status: StatusCode::OK,
        payload: json!({
            "path": request_path,
            "size": size,
            "language": language,
            "truncated": truncated,
            "binary": false,
            "html": html,
        }),
        outcome: if truncated { Outcome::Truncated } else { Outcome::Ok },
    }
}

/// Read at most `MAX_FILE_BYTES` from `path`. Returns the head bytes; the caller
/// compares the returned length against the on-disk size to detect truncation.
fn read_capped(path: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    let mut buf = Vec::new();
    // +1 over the cap so a file EXACTLY at the cap isn't falsely flagged
    // truncated; we then trim back to the cap.
    file.take(MAX_FILE_BYTES as u64 + 1).read_to_end(&mut buf)?;
    buf.truncate(MAX_FILE_BYTES);
    Ok(buf)
}

// ─── HTTP handler ─────────────────────────────────────────────────────────────

/// GET /api/file?path=<relpath> → `{ path, size, language, truncated, binary, html }`
pub async fn get_file_preview(
    Extension(state): Extension<AppState>,
    Query(query): Query<FileQuery>,
) -> Response {
    let request_path = match query.path {
        Some(p) if !p.is_empty() => p,
        _ => {
            tracing::info!(
                target: "architext_serve::file_preview",
                outcome = Outcome::BadRequest.as_str(),
                "GET /api/file missing path"
            );
            return json_response(
                StatusCode::BAD_REQUEST,
                json!({ "error": "missing path query parameter" }),
            );
        }
    };

    // Default-load is cached internally by syntect after first build; we load
    // per request for simplicity (the preview pane is interactive, not hot).
    let syntax_set = SyntaxSet::load_defaults_newlines();
    let theme_set = ThemeSet::load_defaults();

    let result = build_preview(&state.target_dir, &request_path, &syntax_set, &theme_set);

    tracing::info!(
        target: "architext_serve::file_preview",
        path = %request_path,
        outcome = result.outcome.as_str(),
        status = result.status.as_u16(),
        "GET /api/file resolved"
    );

    json_response(result.status, result.payload)
}

/// Serialize `payload` with `no-store` + JSON content-type.
fn json_response(status: StatusCode, payload: Value) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert("cache-control", HeaderValue::from_static("no-store"));
    headers.insert(
        "content-type",
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    let body = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
    (status, headers, body).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn sets() -> (SyntaxSet, ThemeSet) {
        (SyntaxSet::load_defaults_newlines(), ThemeSet::load_defaults())
    }

    #[test]
    fn valid_text_file_is_highlighted() {
        let td = TempDir::new().unwrap();
        fs::write(td.path().join("hello.rs"), b"fn main() { let x = 1; }\n").unwrap();
        let (ss, ts) = sets();
        let r = build_preview(td.path(), "hello.rs", &ss, &ts);
        assert_eq!(r.status, StatusCode::OK);
        assert_eq!(r.payload["binary"], json!(false));
        assert_eq!(r.payload["truncated"], json!(false));
        // Rust syntax picked by extension.
        assert_eq!(r.payload["language"].as_str(), Some("Rust"));
        // Inline-styled HTML produced.
        let html = r.payload["html"].as_str().unwrap();
        assert!(html.contains("<pre"), "expected highlighted <pre> html");
        assert!(html.contains("style="), "expected inline styles");
        assert!(matches!(r.outcome, Outcome::Ok));
    }

    #[test]
    fn unknown_extension_falls_back_to_plain_text() {
        let td = TempDir::new().unwrap();
        fs::write(td.path().join("notes.zzz"), b"just some text\n").unwrap();
        let (ss, ts) = sets();
        let r = build_preview(td.path(), "notes.zzz", &ss, &ts);
        assert_eq!(r.status, StatusCode::OK);
        assert_eq!(r.payload["binary"], json!(false));
        assert_eq!(r.payload["language"].as_str(), Some("Plain Text"));
    }

    #[test]
    fn path_traversal_is_denied() {
        let td = TempDir::new().unwrap();
        let (ss, ts) = sets();
        let r = build_preview(td.path(), "../../etc/passwd", &ss, &ts);
        assert_eq!(r.status, StatusCode::BAD_REQUEST);
        assert!(matches!(r.outcome, Outcome::Denied));
        assert!(r.payload["html"].is_null());
    }

    #[test]
    fn missing_file_is_404() {
        let td = TempDir::new().unwrap();
        let (ss, ts) = sets();
        let r = build_preview(td.path(), "does/not/exist.rs", &ss, &ts);
        assert_eq!(r.status, StatusCode::NOT_FOUND);
        assert!(matches!(r.outcome, Outcome::NotFound));
    }

    #[test]
    fn directory_is_404_not_file() {
        let td = TempDir::new().unwrap();
        fs::create_dir(td.path().join("subdir")).unwrap();
        let (ss, ts) = sets();
        let r = build_preview(td.path(), "subdir", &ss, &ts);
        assert_eq!(r.status, StatusCode::NOT_FOUND);
        assert!(matches!(r.outcome, Outcome::NotFound));
    }

    #[test]
    fn oversized_file_is_truncated() {
        let td = TempDir::new().unwrap();
        // One byte over the cap.
        let big = vec![b'a'; MAX_FILE_BYTES + 1];
        fs::write(td.path().join("big.txt"), &big).unwrap();
        let (ss, ts) = sets();
        let r = build_preview(td.path(), "big.txt", &ss, &ts);
        assert_eq!(r.status, StatusCode::OK);
        assert_eq!(r.payload["truncated"], json!(true));
        assert_eq!(r.payload["binary"], json!(false));
        // size reports the FULL on-disk size, not the truncated read length.
        assert_eq!(r.payload["size"].as_u64(), Some((MAX_FILE_BYTES + 1) as u64));
        assert!(matches!(r.outcome, Outcome::Truncated));
    }

    #[test]
    fn file_exactly_at_cap_is_not_truncated() {
        let td = TempDir::new().unwrap();
        let exact = vec![b'a'; MAX_FILE_BYTES];
        fs::write(td.path().join("exact.txt"), &exact).unwrap();
        let (ss, ts) = sets();
        let r = build_preview(td.path(), "exact.txt", &ss, &ts);
        assert_eq!(r.payload["truncated"], json!(false));
        assert!(matches!(r.outcome, Outcome::Ok));
    }

    #[test]
    fn binary_file_is_flagged() {
        let td = TempDir::new().unwrap();
        // Contains a NUL byte → binary.
        fs::write(td.path().join("blob.bin"), [0x00, 0x01, 0x02, 0xff]).unwrap();
        let (ss, ts) = sets();
        let r = build_preview(td.path(), "blob.bin", &ss, &ts);
        assert_eq!(r.status, StatusCode::OK);
        assert_eq!(r.payload["binary"], json!(true));
        assert!(r.payload["html"].is_null());
        assert!(matches!(r.outcome, Outcome::Binary));
    }

    #[test]
    fn invalid_utf8_is_binary() {
        let td = TempDir::new().unwrap();
        // Invalid UTF-8 (lone continuation byte), no NUL.
        fs::write(td.path().join("bad.txt"), [0x66, 0x6f, 0x80, 0x6f]).unwrap();
        let (ss, ts) = sets();
        let r = build_preview(td.path(), "bad.txt", &ss, &ts);
        assert_eq!(r.payload["binary"], json!(true));
        assert!(r.payload["html"].is_null());
        assert!(matches!(r.outcome, Outcome::Binary));
    }
}
