//! Handler for SPA static file serving.
//!
//! `GET /` → serve `index.html` from viewer dist dir.
//! `GET /{*path}` → serve the matching file if it exists; else fall back to
//!   `index.html` for SPA routes. Unknown `/api/*` routes return 404 JSON.
//!
//! Port of the JS handler in `createViewerRequestHandler`:
//!   ```js
//!   if (url.pathname.startsWith("/api/")) {
//!     sendJson(response, 404, { error: `Unknown Architext API route: ${url.pathname}` });
//!     return;
//!   }
//!   const assetPath = url.pathname === "/" ? "index.html" : url.pathname;
//!   const assetFile = safeJoin(viewerDistDir, assetPath);
//!   const assetStat = assetFile ? await stat(assetFile).catch(() => null) : null;
//!   await sendFile(response, assetStat?.isFile() ? assetFile : path.join(viewerDistDir, "index.html"));
//!   ```

use axum::{
    extract::{Extension, OriginalUri, Path},
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

use crate::content_type::content_type_for_path;
use crate::safe_join::safe_join;
use crate::AppState;

/// GET / — serve index.html
pub async fn get_root(Extension(state): Extension<AppState>) -> Response {
    serve_static_asset("index.html", &state).await
}

/// GET /*path — serve asset, SPA fallback, or /api/* 404 JSON.
pub async fn get_asset(
    Path(sub_path): Path<String>,
    OriginalUri(uri): OriginalUri,
    Extension(state): Extension<AppState>,
) -> Response {
    let full_path = uri.path();

    // /api/* paths that made it here (past the specific /api/session, /api/plan/:hash routes)
    // are unknown API routes → 404 JSON (matches JS).
    if full_path.starts_with("/api/") {
        let body = json!({ "error": format!("Unknown Architext API route: {full_path}") });
        return (StatusCode::NOT_FOUND, Json(body)).into_response();
    }

    serve_static_asset(&sub_path, &state).await
}

async fn serve_static_asset(asset_path: &str, state: &AppState) -> Response {
    // Try safe_join; on traversal/decode failure fall back to index.html (matches JS).
    let resolved = safe_join(&state.dist_dir, asset_path);

    // Check if it's an actual file
    let actual_path = match resolved {
        Some(p) if tokio::fs::metadata(&p).await.map(|m| m.is_file()).unwrap_or(false) => p,
        _ => {
            // SPA fallback: serve index.html
            state.dist_dir.join("index.html")
        }
    };

    let body = match tokio::fs::read(&actual_path).await {
        Ok(b) => b,
        Err(_) => {
            return (StatusCode::NOT_FOUND, "Not found").into_response();
        }
    };

    let content_type = content_type_for_path(&actual_path);
    let mut response = Response::new(axum::body::Body::from(body));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        "content-type",
        HeaderValue::from_static(content_type),
    );
    response
}
