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

use std::path::Path as FsPath;

use crate::content_type::content_type_for_path;
use crate::embedded_viewer::embedded_asset;
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
    // 1. On-disk dist FIRST: the ARCHITEXT_VIEWER_DIST override, an npm co-located
    //    `<exe_dir>/dist`, or a dev source tree all win over the embedded copy.
    if let Some(p) = safe_join(&state.dist_dir, asset_path) {
        if tokio::fs::metadata(&p).await.map(|m| m.is_file()).unwrap_or(false) {
            if let Ok(body) = tokio::fs::read(&p).await {
                return asset_response(&p, body);
            }
        }
    }

    // 2. Embedded viewer: makes a standalone native binary self-contained.
    if let Some(bytes) = embedded_asset(asset_path) {
        return asset_response(FsPath::new(asset_path), bytes.into_owned());
    }

    // 3. SPA fallback → index.html (on-disk, then embedded).
    let index = state.dist_dir.join("index.html");
    if let Ok(body) = tokio::fs::read(&index).await {
        return asset_response(&index, body);
    }
    if let Some(bytes) = embedded_asset("index.html") {
        return asset_response(FsPath::new("index.html"), bytes.into_owned());
    }
    (StatusCode::NOT_FOUND, "Not found").into_response()
}

fn asset_response(path: &FsPath, body: Vec<u8>) -> Response {
    let content_type = content_type_for_path(path);
    let mut response = Response::new(axum::body::Body::from(body));
    *response.status_mut() = StatusCode::OK;
    response
        .headers_mut()
        .insert("content-type", HeaderValue::from_static(content_type));
    response
}
